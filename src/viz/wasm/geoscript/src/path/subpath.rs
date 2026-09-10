use std::{cell::OnceCell, cmp::Ordering, hash::Hasher};

use nalgebra::Matrix3;

use super::{centroid::segments_signed_area, segment::*, shared_vec::SharedVec};
use crate::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LastCtrl {
  Quad(Vec2),
  Cubic(Vec2),
}

/// One concrete subpath. `anchors[i]` flags the joint at `segments[i].start`; for a closed
/// subpath that joint doubles as the end joint. `end`/`total_length` are stored so pen ops can
/// extend the tail without reading (and thereby freezing) the shared buffers.
#[derive(Clone, Debug)]
pub struct Subpath {
  pub(crate) start: Vec2,
  end: Vec2,
  total_length: f32,
  pub(crate) segments: SharedVec<PathSegment>,
  pub(crate) cumulative_lengths: SharedVec<f32>,
  pub(crate) closed: bool,
  pub(crate) anchors: SharedVec<bool>,
  /// Producer-marked anchors (booleans, offsets, discretization) are mandatory boundaries when
  /// resampling under a point budget; authored joints are left to the adaptive sampler.
  pub(crate) explicit_anchors: bool,
  pub(crate) last_ctrl: Option<LastCtrl>,
  signed_area: OnceCell<f32>,
}

impl Subpath {
  pub(crate) fn new(
    start: Vec2,
    segments: Vec<PathSegment>,
    closed: bool,
    anchors: Vec<bool>,
    explicit_anchors: bool,
    last_ctrl: Option<LastCtrl>,
  ) -> Self {
    let mut total = 0.;
    let cumulative_lengths = segments
      .iter()
      .map(|seg| {
        total += seg.length();
        total
      })
      .collect();
    let end = segments.last().map(|s| s.end()).unwrap_or(start);
    Self::from_parts(
      start,
      end,
      segments.into(),
      cumulative_lengths,
      closed,
      anchors.into(),
      explicit_anchors,
      last_ctrl,
    )
  }

  pub(crate) fn from_parts(
    start: Vec2,
    end: Vec2,
    segments: SharedVec<PathSegment>,
    cumulative_lengths: SharedVec<f32>,
    closed: bool,
    anchors: SharedVec<bool>,
    explicit_anchors: bool,
    last_ctrl: Option<LastCtrl>,
  ) -> Self {
    debug_assert_eq!(anchors.len(), segments.len());
    debug_assert_eq!(cumulative_lengths.len(), segments.len());
    Self {
      start,
      end,
      total_length: cumulative_lengths.last_copied().unwrap_or(0.),
      segments,
      cumulative_lengths,
      closed,
      anchors,
      explicit_anchors,
      last_ctrl,
      signed_area: OnceCell::new(),
    }
  }

  pub(crate) fn total_length(&self) -> f32 {
    self.total_length
  }

  pub(crate) fn is_degenerate(&self) -> bool {
    self.total_length() <= LENGTH_EPSILON
  }

  pub(crate) fn end(&self) -> Vec2 {
    self.end
  }

  pub(crate) fn sample_by_length(&self, length: f32) -> Vec2 {
    let lens = self.cumulative_lengths.as_slice();
    let idx = lens
      .partition_point(|&len| len < length)
      .min(lens.len() - 1);
    self.sample_in_segment(idx, length)
  }

  /// `sample_by_length` that tries `*hint` (the previous result) and its neighbors before
  /// binary searching, so runs of nearby lengths locate in O(1).
  pub(crate) fn sample_by_length_hinted(&self, length: f32, hint: &mut usize) -> Vec2 {
    let lens = self.cumulative_lengths.as_slice();
    let n = lens.len();
    let h = (*hint).min(n - 1);
    let below = |i: usize| i == 0 || lens[i - 1] < length;
    let within = |i: usize| below(i) && !(lens[i] < length);
    let idx = if within(h) {
      h
    } else if h + 1 < n && within(h + 1) {
      h + 1
    } else if h > 0 && within(h - 1) {
      h - 1
    } else if below(h) {
      (h + 1 + lens[(h + 1).min(n)..].partition_point(|&l| l < length)).min(n - 1)
    } else {
      lens[..h].partition_point(|&l| l < length)
    };
    *hint = idx;
    self.sample_in_segment(idx, length)
  }

  fn sample_in_segment(&self, idx: usize, length: f32) -> Vec2 {
    let lens = self.cumulative_lengths.as_slice();
    let seg_start_len = if idx == 0 { 0.0 } else { lens[idx - 1] };
    let seg = &self.segments.as_slice()[idx];
    let seg_len = seg.length();
    if seg_len <= LENGTH_EPSILON {
      return seg.end();
    }
    seg.sample_by_length((length - seg_start_len).clamp(0.0, seg_len))
  }

  pub(crate) fn sample_t(&self, t: f32) -> Vec2 {
    self.sample_by_length(t * self.total_length())
  }

  pub(crate) fn sample_t_hinted(&self, t: f32, hint: &mut usize) -> Vec2 {
    self.sample_by_length_hinted(t * self.total_length(), hint)
  }

  /// Anchored joints as local `t`; `0`/`1` iff the start joint is anchored (open ends always).
  pub(crate) fn critical_t_values(&self) -> Vec<f32> {
    if self.is_degenerate() {
      return Vec::new();
    }
    let total = self.total_length();
    let mut out = Vec::with_capacity(self.segments.len() + 1);
    if self.anchors[0] {
      out.push(0.);
    }
    for i in 1..self.segments.len() {
      if self.anchors[i] {
        out.push((self.cumulative_lengths[i - 1] / total).clamp(0., 1.));
      }
    }
    if !self.closed || self.anchors[0] {
      out.push(1.);
    }
    out
  }

  /// Mandatory boundaries for budget-limited adaptive resampling.
  pub(crate) fn resample_boundaries(&self) -> Vec<f32> {
    if !self.explicit_anchors {
      return vec![0., 1.];
    }
    let mut local = self.critical_t_values();
    if local.iter().all(|&t| t > 1e-6) {
      local.push(0.);
    }
    if local.iter().all(|&t| t < 1. - 1e-6) {
      local.push(1.);
    }
    local.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    local
  }

  pub(crate) fn signed_area(&self) -> f32 {
    *self.signed_area.get_or_init(|| {
      if self.closed {
        segments_signed_area(self.segments.iter())
      } else {
        0.
      }
    })
  }

  /// Whether the tangent's left-perpendicular points outward (CW winding).
  pub(crate) fn inward_flip(&self) -> bool {
    self.closed && self.signed_area() < 0.
  }

  pub(crate) fn reversed(&self) -> Subpath {
    let n = self.segments.len();
    let segments: Vec<_> = self.segments.iter().rev().map(|s| s.reversed()).collect();
    let mut anchors = Vec::with_capacity(n);
    if n > 0 {
      anchors.push(if self.closed { self.anchors[0] } else { true });
      anchors.extend((1..n).map(|i| self.anchors[n - i]));
    }
    let start = segments
      .first()
      .map(|s| s.start_point())
      .unwrap_or(self.start);
    Subpath::new(
      start,
      segments,
      self.closed,
      anchors,
      self.explicit_anchors,
      None,
    )
  }

  pub(crate) fn transformed(&self, m: &Matrix3<f32>) -> Subpath {
    let mut segments = Vec::with_capacity(self.segments.len());
    let mut anchors = Vec::with_capacity(self.segments.len());
    for (seg, &anchor) in self.segments.iter().zip(&self.anchors) {
      let before = segments.len();
      seg.transform_into(m, &mut segments);
      anchors.push(anchor);
      anchors.resize(segments.len(), false);
      debug_assert!(segments.len() > before);
    }
    let tx = |p: Vec2| apply_transform_to_point(m, p);
    let last_ctrl = self.last_ctrl.map(|c| match c {
      LastCtrl::Quad(p) => LastCtrl::Quad(tx(p)),
      LastCtrl::Cubic(p) => LastCtrl::Cubic(tx(p)),
    });
    Subpath::new(
      tx(self.start),
      segments,
      self.closed,
      anchors,
      self.explicit_anchors,
      last_ctrl,
    )
  }

  /// Open copy closed with a line back to `start` (no-op for closed or empty subpaths).
  pub(crate) fn closed_copy(&self) -> Subpath {
    if self.closed || self.segments.is_empty() {
      return self.clone();
    }
    let mut segments = self.segments.clone();
    let mut cumulative_lengths = self.cumulative_lengths.clone();
    let mut anchors = self.anchors.clone();
    let closing = line_segment(self.end, self.start);
    let mut end = self.end;
    if closing.length() > LENGTH_EPSILON {
      cumulative_lengths.push(self.total_length + closing.length());
      segments.push(closing);
      anchors.push(true);
      end = self.start;
    }
    Subpath::from_parts(
      self.start,
      end,
      segments,
      cumulative_lengths,
      true,
      anchors,
      self.explicit_anchors,
      None,
    )
  }

  /// Adaptive flattening; segment endpoints are emitted verbatim so corners stay exact.
  #[cfg(test)]
  pub(crate) fn sample_points(
    &self,
    angle_tolerance: f32,
    max_sagitta: f32,
    include_end: bool,
  ) -> Vec<Vec2> {
    self
      .sample_points_tagged(angle_tolerance, max_sagitta, include_end)
      .0
  }

  pub(crate) fn sample_points_tagged(
    &self,
    angle_tolerance: f32,
    max_sagitta: f32,
    include_end: bool,
  ) -> (Vec<Vec2>, Vec<bool>) {
    if self.is_degenerate() {
      return (Vec::new(), Vec::new());
    }
    let mut points = Vec::with_capacity(self.segments.len() + 1);
    let mut anchors = Vec::with_capacity(self.segments.len() + 1);
    for (seg, &anchor) in self.segments.iter().zip(&self.anchors) {
      let seg_len = seg.length();
      if seg_len <= LENGTH_EPSILON {
        continue;
      }
      points.push(seg.start_point());
      anchors.push(anchor);
      let subdivs = segment_subdivisions(seg, angle_tolerance, max_sagitta);
      for j in 1..subdivs {
        points.push(seg.sample_by_length(seg_len * (j as f32 / subdivs as f32)));
        anchors.push(false);
      }
    }
    if include_end && !points.is_empty() {
      points.push(self.segments.last().unwrap().end());
      anchors.push(!self.closed || self.anchors[0]);
    }
    (points, anchors)
  }

  pub(crate) fn aabb(&self) -> Option<(Vec2, Vec2)> {
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for seg in &self.segments {
      let (smin, smax) = seg.aabb();
      min = min.inf(&smin);
      max = max.sup(&smax);
    }
    (!self.segments.is_empty()).then_some((min, max))
  }

  pub(crate) fn content_hash(&self, h: &mut dyn Hasher) {
    let hv = |h: &mut dyn Hasher, p: &Vec2| {
      h.write_u32(p.x.to_bits());
      h.write_u32(p.y.to_bits());
    };
    hv(h, &self.start);
    h.write_u8(self.closed as u8);
    h.write_u8(self.explicit_anchors as u8);
    for (seg, &a) in self.segments.iter().zip(&self.anchors) {
      h.write_u8(a as u8);
      match seg {
        PathSegment::Line { start, end, .. } => {
          h.write_u8(0);
          hv(h, start);
          hv(h, end);
        }
        PathSegment::Quadratic {
          start, ctrl, end, ..
        } => {
          h.write_u8(1);
          hv(h, start);
          hv(h, ctrl);
          hv(h, end);
        }
        PathSegment::Cubic {
          start,
          ctrl1,
          ctrl2,
          end,
          ..
        } => {
          h.write_u8(2);
          hv(h, start);
          hv(h, ctrl1);
          hv(h, ctrl2);
          hv(h, end);
        }
        PathSegment::Arc {
          end,
          center,
          rx,
          ry,
          cos_phi,
          sin_phi,
          theta_start,
          theta_delta,
          ..
        } => {
          h.write_u8(3);
          hv(h, end);
          hv(h, center);
          for f in [rx, ry, cos_phi, sin_phi, theta_start, theta_delta] {
            h.write_u32(f.to_bits());
          }
        }
      }
    }
  }
}
