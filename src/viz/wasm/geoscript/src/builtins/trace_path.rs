use std::any::Any;
use std::cmp::Ordering;
use std::f32::consts::PI;
use std::rc::Rc;

use fxhash::FxHashMap;
use svgtypes::PathParser;

use crate::{
  ast::{ClosureBody, Expr, FunctionCall, FunctionCallTarget},
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    FUNCTION_ALIASES,
  },
  format_fn_signatures, get_args, AppendOnlyBuffer, ArgRef, ArgType, Callable, CapturedScope,
  Closure, DynamicCallable, ErrorStack, EvalCtx, GetArgsOutput, Scope, Sequence, Sym, Value, Vec2,
  EMPTY_ARGS, EMPTY_KWARGS,
};

const CURVE_TABLE_SAMPLES: usize = 32;
const LENGTH_EPSILON: f32 = 1e-5;

fn extend_bounds(min: &mut Vec2, max: &mut Vec2, p: Vec2) {
  min.x = min.x.min(p.x);
  min.y = min.y.min(p.y);
  max.x = max.x.max(p.x);
  max.y = max.y.max(p.y);
}

#[derive(Clone)]
struct ArcLengthTable {
  cumulative: Vec<f32>,
  total: f32,
}

impl ArcLengthTable {
  fn new(samples: usize, mut sample_fn: impl FnMut(f32) -> Vec2) -> (Self, Vec2, Vec2) {
    let samples = samples.max(1);
    let mut cumulative = Vec::with_capacity(samples + 1);
    let mut total = 0.0;

    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    let mut prev = sample_fn(0.0);
    extend_bounds(&mut min, &mut max, prev);
    cumulative.push(0.0);

    for i in 1..=samples {
      let t = i as f32 / samples as f32;
      let point = sample_fn(t);
      extend_bounds(&mut min, &mut max, point);
      total += (point - prev).norm();
      cumulative.push(total);
      prev = point;
    }

    (Self { cumulative, total }, min, max)
  }

  fn total(&self) -> f32 {
    self.total
  }

  fn param_for_length(&self, length: f32) -> f32 {
    if self.total <= LENGTH_EPSILON {
      return 0.0;
    }
    let target = length.clamp(0.0, self.total);
    let idx = match self
      .cumulative
      .binary_search_by(|val| val.partial_cmp(&target).unwrap_or(Ordering::Less))
    {
      Ok(ix) => ix,
      Err(ix) => ix,
    };
    if idx == 0 {
      return 0.0;
    }
    if idx >= self.cumulative.len() {
      return 1.0;
    }

    let prev = self.cumulative[idx - 1];
    let next = self.cumulative[idx];
    let span = next - prev;
    let alpha = if span <= 0.0 {
      0.0
    } else {
      (target - prev) / span
    };
    let samples = (self.cumulative.len() - 1) as f32;
    let t0 = (idx - 1) as f32 / samples;
    let t1 = idx as f32 / samples;
    t0 + (t1 - t0) * alpha
  }
}

#[derive(Clone)]
enum PathSegment {
  Line {
    start: Vec2,
    end: Vec2,
    length: f32,
  },
  Quadratic {
    start: Vec2,
    ctrl: Vec2,
    end: Vec2,
    table: ArcLengthTable,
  },
  Cubic {
    start: Vec2,
    ctrl1: Vec2,
    ctrl2: Vec2,
    end: Vec2,
    table: ArcLengthTable,
  },
  Arc {
    end: Vec2,
    center: Vec2,
    rx: f32,
    ry: f32,
    cos_phi: f32,
    sin_phi: f32,
    theta_start: f32,
    theta_delta: f32,
    table: ArcLengthTable,
  },
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentInterval {
  pub start: f32,
  pub end: f32,
  pub has_detail: bool,
}

const GUIDE_EPSILON: f32 = 1e-6;

pub(crate) fn normalize_guides(guides: &[f32]) -> Vec<f32> {
  let mut out: Vec<f32> = guides
    .iter()
    .copied()
    .filter(|v| v.is_finite())
    .map(|v| v.clamp(0.0, 1.0))
    .collect();
  out.push(0.0);
  out.push(1.0);
  out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  out.dedup_by(|a, b| (*a - *b).abs() <= GUIDE_EPSILON);
  out
}

fn uniform_samples(count: usize, include_end: bool) -> Vec<f32> {
  if count == 0 {
    return Vec::new();
  }
  if include_end {
    if count == 1 {
      return vec![0.0];
    }
    let denom = (count - 1) as f32;
    return (0..count).map(|i| i as f32 / denom).collect();
  }

  let denom = count as f32;
  (0..count).map(|i| i as f32 / denom).collect()
}

pub(crate) fn build_topology_samples(
  sample_count: usize,
  guides: Option<&[f32]>,
  interval_weights: Option<&[f32]>,
  include_end: bool,
) -> Vec<f32> {
  let Some(guides) = guides else {
    return uniform_samples(sample_count, include_end);
  };

  let guide_points = normalize_guides(guides);

  if guide_points.len() < 2 {
    return uniform_samples(sample_count, include_end);
  }

  let interval_count = guide_points.len() - 1;
  let base_count = if include_end {
    interval_count + 1
  } else {
    interval_count
  };
  let target_count = sample_count.max(base_count);
  let remaining = target_count - base_count;
  if remaining == 0 {
    return if include_end {
      guide_points
    } else {
      guide_points[..guide_points.len() - 1].to_vec()
    };
  }

  let weights = interval_weights.filter(|weights| weights.len() == interval_count);
  let mut spans = Vec::with_capacity(interval_count);
  let mut total_effective = 0.0;
  for (ix, &[start, end]) in guide_points.array_windows::<2>().enumerate() {
    let span = (end - start).max(0.);
    let weight = weights.map(|weights| weights[ix]).unwrap_or(1.).max(0.);
    let effective = span * weight;
    spans.push((span, effective));
    total_effective += effective;
  }

  if total_effective <= GUIDE_EPSILON {
    total_effective = 0.0;
    for (span, effective) in spans.iter_mut() {
      *effective = *span;
      total_effective += *effective;
    }
  }

  let mut allocations = Vec::with_capacity(interval_count);
  let mut remainders: Vec<(f32, usize)> = Vec::with_capacity(interval_count);
  let mut assigned = 0usize;

  for (ix, (span, effective)) in spans.iter().enumerate() {
    if *span <= 0.0 {
      allocations.push(0);
      remainders.push((0.0, ix));
      continue;
    }
    let exact = if total_effective > 0.0 {
      (remaining as f32) * (effective / total_effective)
    } else {
      0.0
    };
    let count = exact.floor() as usize;
    assigned += count;
    allocations.push(count);
    remainders.push((exact - count as f32, ix));
  }

  let mut leftover = remaining.saturating_sub(assigned);
  remainders.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
  for (_, ix) in remainders {
    if leftover == 0 {
      break;
    }
    allocations[ix] += 1;
    leftover -= 1;
  }

  let mut samples = Vec::with_capacity(target_count);
  for (ix, &[start, end]) in guide_points.array_windows::<2>().enumerate() {
    samples.push(start);

    let count = allocations[ix];
    if count == 0 {
      continue;
    }
    let step = (end - start) / ((count + 1) as f32);
    for j in 1..=count {
      samples.push(start + step * (j as f32));
    }
  }

  if include_end {
    samples.push(*guide_points.last().unwrap_or(&1.));
  }

  samples
}

pub(crate) fn build_interval_weights(
  guides: &[f32],
  sampler_intervals: &[Vec<SegmentInterval>],
) -> Option<Vec<f32>> {
  if guides.len() < 2 || sampler_intervals.is_empty() {
    return None;
  }

  let mut indices = vec![0usize; sampler_intervals.len()];
  let mut weights = Vec::with_capacity(guides.len() - 1);
  for &[start, end] in guides.array_windows::<2>() {
    let mid = (start + end) * 0.5;
    let mut all_detail = true;

    for (sampler_ix, intervals) in sampler_intervals.iter().enumerate() {
      if intervals.is_empty() {
        all_detail = false;
        continue;
      }

      let mut idx = indices[sampler_ix];
      while idx + 1 < intervals.len() && mid > intervals[idx].end + GUIDE_EPSILON {
        idx += 1;
      }
      indices[sampler_ix] = idx;

      if !intervals[idx].has_detail {
        all_detail = false;
      }
    }

    weights.push(if all_detail { 1.0 } else { 0.0 });
  }

  Some(weights)
}

impl PathSegment {
  fn translate(&mut self, offset: Vec2) {
    match self {
      PathSegment::Line { start, end, .. } => {
        *start = *start + offset;
        *end = *end + offset;
      }
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => {
        *start = *start + offset;
        *ctrl = *ctrl + offset;
        *end = *end + offset;
      }
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => {
        *start = *start + offset;
        *ctrl1 = *ctrl1 + offset;
        *ctrl2 = *ctrl2 + offset;
        *end = *end + offset;
      }
      PathSegment::Arc { center, end, .. } => {
        *center = *center + offset;
        *end = *end + offset;
      }
    }
  }

  fn length(&self) -> f32 {
    match self {
      PathSegment::Line { length, .. } => *length,
      PathSegment::Quadratic { table, .. } => table.total(),
      PathSegment::Cubic { table, .. } => table.total(),
      PathSegment::Arc { table, .. } => table.total(),
    }
  }

  fn has_detail(&self) -> bool {
    !matches!(self, PathSegment::Line { .. })
  }

  fn end(&self) -> Vec2 {
    match self {
      PathSegment::Line { end, .. } => *end,
      PathSegment::Quadratic { end, .. } => *end,
      PathSegment::Cubic { end, .. } => *end,
      PathSegment::Arc { end, .. } => *end,
    }
  }

  fn sample_by_length(&self, length: f32) -> Vec2 {
    match self {
      PathSegment::Line {
        start,
        end,
        length: seg_len,
      } => {
        if *seg_len <= LENGTH_EPSILON {
          return *end;
        }
        let t = (length / *seg_len).clamp(0.0, 1.0);
        *start + (*end - *start) * t
      }
      PathSegment::Quadratic {
        start,
        ctrl,
        end,
        table,
      } => {
        let t = table.param_for_length(length);
        quadratic_bezier(*start, *ctrl, *end, t)
      }
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        table,
      } => {
        let t = table.param_for_length(length);
        cubic_bezier(*start, *ctrl1, *ctrl2, *end, t)
      }
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        theta_delta,
        table,
        ..
      } => {
        let t = table.param_for_length(length);
        arc_point(
          *center,
          *rx,
          *ry,
          *cos_phi,
          *sin_phi,
          *theta_start,
          *theta_delta,
          t,
        )
      }
    }
  }
}

#[derive(Clone)]
pub(crate) struct PathSubpath {
  segments: Vec<PathSegment>,
  cumulative_lengths: Vec<f32>,
  total_length: f32,
  closed: bool,
}

struct SubpathBuilder {
  start: Vec2,
  current: Vec2,
  segments: Vec<PathSegment>,
  closed: bool,
  last_cubic_ctrl: Option<Vec2>,
  last_quad_ctrl: Option<Vec2>,
}

impl SubpathBuilder {
  fn new(start: Vec2) -> Self {
    Self {
      start,
      current: start,
      segments: Vec::new(),
      closed: false,
      last_cubic_ctrl: None,
      last_quad_ctrl: None,
    }
  }
}

impl PathSubpath {
  fn new(segments: Vec<PathSegment>, closed: bool) -> Option<Self> {
    if segments.is_empty() {
      return None;
    }
    let mut cumulative_lengths = Vec::with_capacity(segments.len());
    let mut total_length = 0.0;
    for segment in &segments {
      total_length += segment.length();
      cumulative_lengths.push(total_length);
    }
    if total_length <= LENGTH_EPSILON {
      return None;
    }
    Some(Self {
      segments,
      cumulative_lengths,
      total_length,
      closed,
    })
  }

  fn sample_by_length(&self, length: f32) -> Vec2 {
    let mut idx = match self
      .cumulative_lengths
      .binary_search_by(|len| len.partial_cmp(&length).unwrap_or(Ordering::Less))
    {
      Ok(ix) => ix,
      Err(ix) => ix,
    };
    if idx >= self.segments.len() {
      idx = self.segments.len() - 1;
    }
    let seg_start_len = if idx == 0 {
      0.0
    } else {
      self.cumulative_lengths[idx - 1]
    };
    let seg = &self.segments[idx];
    let seg_len = seg.length();
    if seg_len <= LENGTH_EPSILON {
      return seg.end();
    }
    let local_len = (length - seg_start_len).clamp(0.0, seg_len);
    seg.sample_by_length(local_len)
  }

  pub(crate) fn critical_t_values(&self) -> Vec<f32> {
    if self.segments.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Vec::new();
    }

    let mut out = Vec::with_capacity(self.cumulative_lengths.len() + 1);
    out.push(0.0);
    for len in &self.cumulative_lengths {
      out.push((len / self.total_length).clamp(0.0, 1.0));
    }
    out
  }

  pub(crate) fn segment_intervals(&self) -> Vec<SegmentInterval> {
    if self.segments.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Vec::new();
    }

    let mut intervals = Vec::with_capacity(self.segments.len());
    let mut prev = 0.0f32;
    for (seg, cum_len) in self.segments.iter().zip(self.cumulative_lengths.iter()) {
      let end = (cum_len / self.total_length).clamp(0., 1.);
      let start = prev.clamp(0., 1.);
      intervals.push(SegmentInterval {
        start,
        end,
        has_detail: seg.has_detail(),
      });
      prev = end;
    }

    intervals
  }

  pub(crate) fn is_closed(&self) -> bool {
    self.closed
  }

  pub(crate) fn total_length(&self) -> f32 {
    self.total_length
  }
}

pub struct PathTracerCallable {
  interned_t_kwarg: Sym,
  pub subpaths: Vec<PathSubpath>,
  pub subpath_cumulative_lengths: Vec<f32>,
  pub total_length: f32,
  pub reverse: bool,
  override_critical_points: Option<Vec<f32>>,
}

impl PathTracerCallable {
  pub fn new(
    closed: bool,
    center: bool,
    reverse: bool,
    draw_cmds: Vec<DrawCommand>,
    interned_t_kwarg: Sym,
  ) -> Self {
    let mut subpaths: Vec<PathSubpath> = Vec::new();
    let mut builder: Option<SubpathBuilder> = None;
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    fn finalize_subpath(
      builder: &mut Option<SubpathBuilder>,
      force_close: bool,
      min: &mut Vec2,
      max: &mut Vec2,
      out: &mut Vec<PathSubpath>,
    ) {
      let Some(mut builder) = builder.take() else {
        return;
      };

      if force_close && !builder.closed {
        let cur = builder.current;
        let start = builder.start;
        extend_bounds(min, max, cur);
        extend_bounds(min, max, start);
        let length = (start - cur).norm();
        if length > LENGTH_EPSILON {
          builder.segments.push(PathSegment::Line {
            start: cur,
            end: start,
            length,
          });
        }
        builder.current = start;
        builder.closed = true;
      }

      if let Some(subpath) = PathSubpath::new(builder.segments, builder.closed) {
        out.push(subpath);
      }
    }

    fn get_or_create_builder(
      builder: &mut Option<SubpathBuilder>,
      start: Vec2,
    ) -> &mut SubpathBuilder {
      if builder.is_none() {
        *builder = Some(SubpathBuilder::new(start));
      }
      builder.as_mut().unwrap()
    }

    for cmd in draw_cmds {
      match cmd {
        DrawCommand::MoveTo(pos) => {
          finalize_subpath(&mut builder, closed, &mut min, &mut max, &mut subpaths);
          extend_bounds(&mut min, &mut max, pos);
          builder = Some(SubpathBuilder::new(pos));
        }
        DrawCommand::LineTo(pos) => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0.0, 0.0));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          extend_bounds(&mut min, &mut max, start);
          extend_bounds(&mut min, &mut max, pos);
          let length = (pos - start).norm();
          if length > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Line {
              start,
              end: pos,
              length,
            });
          }
          builder.current = pos;
          builder.last_cubic_ctrl = None;
          builder.last_quad_ctrl = None;
        }
        DrawCommand::QuadraticBezier { ctrl, to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0.0, 0.0));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            quadratic_bezier(start, ctrl, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Quadratic {
              start,
              ctrl,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_quad_ctrl = Some(ctrl);
          builder.last_cubic_ctrl = None;
        }
        DrawCommand::SmoothQuadraticBezier { to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0., 0.));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let ctrl = match builder.last_quad_ctrl {
            Some(last_ctrl) => start + (start - last_ctrl),
            None => start,
          };
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            quadratic_bezier(start, ctrl, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Quadratic {
              start,
              ctrl,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_quad_ctrl = Some(ctrl);
          builder.last_cubic_ctrl = None;
        }
        DrawCommand::CubicBezier { ctrl1, ctrl2, to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0., 0.));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            cubic_bezier(start, ctrl1, ctrl2, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Cubic {
              start,
              ctrl1,
              ctrl2,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_cubic_ctrl = Some(ctrl2);
          builder.last_quad_ctrl = None;
        }
        DrawCommand::SmoothCubicBezier { ctrl2, to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0., 0.));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let ctrl1 = match builder.last_cubic_ctrl {
            Some(last_ctrl) => start + (start - last_ctrl),
            None => start,
          };
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            cubic_bezier(start, ctrl1, ctrl2, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Cubic {
              start,
              ctrl1,
              ctrl2,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_cubic_ctrl = Some(ctrl2);
          builder.last_quad_ctrl = None;
        }
        DrawCommand::Arc {
          rx,
          ry,
          x_axis_rotation,
          large_arc,
          sweep,
          to,
        } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0.0, 0.0));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          if let Some((segment, tmin, tmax)) =
            build_arc_segment(start, to, rx, ry, x_axis_rotation, large_arc, sweep)
          {
            extend_bounds(&mut min, &mut max, tmin);
            extend_bounds(&mut min, &mut max, tmax);
            if segment.length() > LENGTH_EPSILON {
              builder.segments.push(segment);
            }
          }
          builder.current = to;
          builder.last_cubic_ctrl = None;
          builder.last_quad_ctrl = None;
        }
        DrawCommand::Close => {
          if let Some(builder) = builder.as_mut() {
            let cur = builder.current;
            let first = builder.start;
            extend_bounds(&mut min, &mut max, cur);
            extend_bounds(&mut min, &mut max, first);
            let length = (first - cur).norm();
            if length > LENGTH_EPSILON {
              builder.segments.push(PathSegment::Line {
                start: cur,
                end: first,
                length,
              });
            }
            builder.current = first;
            builder.closed = true;
            builder.last_cubic_ctrl = None;
            builder.last_quad_ctrl = None;
          }
        }
      }
    }

    finalize_subpath(&mut builder, closed, &mut min, &mut max, &mut subpaths);

    if center && min.x <= max.x {
      let center_pt = (min + max) * 0.5;
      let offset = -center_pt;
      for subpath in &mut subpaths {
        for segment in &mut subpath.segments {
          segment.translate(offset);
        }
      }
    }

    let mut subpath_cumulative_lengths = Vec::with_capacity(subpaths.len());
    let mut total_length = 0.0;
    for subpath in &subpaths {
      total_length += subpath.total_length;
      subpath_cumulative_lengths.push(total_length);
    }

    Self {
      interned_t_kwarg,
      subpaths,
      subpath_cumulative_lengths,
      total_length,
      reverse,
      override_critical_points: None,
    }
  }

  #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
  pub fn new_with_critical_points(
    closed: bool,
    center: bool,
    reverse: bool,
    draw_cmds: Vec<DrawCommand>,
    interned_t_kwarg: Sym,
    override_critical_points: Option<Vec<f32>>,
  ) -> Self {
    let mut tracer = Self::new(closed, center, reverse, draw_cmds, interned_t_kwarg);
    tracer.override_critical_points = override_critical_points;
    tracer
  }

  /// "Critical t values" refer to points at which sharp features occur in the path, such as where
  /// two line segments intersect.  These are used to provide extra information when sampling to
  /// avoid aliasing missing these sharp details.
  ///
  /// When `reverse` is enabled, these values are transformed via `1 - t` so that they still
  /// correspond to the same physical points on the path when using the reversed sampling.
  pub(crate) fn critical_t_values(&self) -> Vec<f32> {
    if let Some(ref override_cps) = self.override_critical_points {
      let mut out: Vec<f32> = override_cps
        .iter()
        .copied()
        .map(|t| t.clamp(0.0, 1.0))
        .collect();
      if self.reverse {
        for t in &mut out {
          *t = 1.0 - *t;
        }
        out.reverse();
      }
      return out;
    }

    if self.subpaths.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Vec::new();
    }

    let mut out = Vec::with_capacity(self.subpath_cumulative_lengths.len() + 1);
    out.push(0.0);
    let mut offset = 0.0;
    for subpath in &self.subpaths {
      for len in &subpath.cumulative_lengths {
        out.push(((offset + len) / self.total_length).clamp(0.0, 1.0));
      }
      offset += subpath.total_length;
    }

    if self.reverse {
      // Transform all t values via `1 - t` and reverse the order to maintain sorted order
      for t in &mut out {
        *t = 1.0 - *t;
      }
      out.reverse();
    }

    out
  }

  pub(crate) fn segment_intervals(&self) -> Vec<SegmentInterval> {
    if self.subpaths.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Vec::new();
    }

    let mut intervals = Vec::new();
    let mut prev = 0.0f32;
    let mut offset = 0.0f32;
    for subpath in &self.subpaths {
      for (seg, cum_len) in subpath
        .segments
        .iter()
        .zip(subpath.cumulative_lengths.iter())
      {
        let end = ((offset + cum_len) / self.total_length).clamp(0.0, 1.0);
        let start = prev.clamp(0.0, 1.0);
        intervals.push(SegmentInterval {
          start,
          end,
          has_detail: seg.has_detail(),
        });
        prev = end;
      }
      offset += subpath.total_length;
    }

    if self.reverse {
      for interval in &mut intervals {
        let start = (1.0 - interval.end).clamp(0.0, 1.0);
        let end = (1.0 - interval.start).clamp(0.0, 1.0);
        interval.start = start;
        interval.end = end;
      }
      intervals.reverse();
    }

    intervals
  }

  fn sample(&self, t: f32) -> Result<Vec2, ErrorStack> {
    if self.subpaths.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Err(ErrorStack::new(
        "trace_path path has no drawable segments to sample",
      ));
    }

    // When reverse is true, sample at (1 - t) to walk the path backwards
    let t = if self.reverse { 1.0 - t } else { t };

    let target = t * self.total_length;
    let mut subpath_ix = match self
      .subpath_cumulative_lengths
      .binary_search_by(|len| len.partial_cmp(&target).unwrap_or(Ordering::Less))
    {
      Ok(ix) => ix,
      Err(ix) => ix,
    };
    if subpath_ix >= self.subpaths.len() {
      subpath_ix = self.subpaths.len() - 1;
    }
    let subpath_start_len = if subpath_ix == 0 {
      0.0
    } else {
      self.subpath_cumulative_lengths[subpath_ix - 1]
    };
    let subpath = &self.subpaths[subpath_ix];
    let local_target = (target - subpath_start_len).clamp(0.0, subpath.total_length);
    Ok(subpath.sample_by_length(local_target))
  }

  pub(crate) fn from_subpath(subpath: PathSubpath, interned_t_kwarg: Sym, reverse: bool) -> Self {
    let total_length = subpath.total_length;
    Self {
      interned_t_kwarg,
      subpaths: vec![subpath],
      subpath_cumulative_lengths: vec![total_length],
      total_length,
      reverse,
      override_critical_points: None,
    }
  }
}

/// A lazy sequence that yields new `PathTracerCallable`` instances for each subpath
pub(crate) struct SubpathsSeq {
  tracer: Rc<Callable>,
}

impl std::fmt::Debug for SubpathsSeq {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "SubpathsSeq {{ {} subpaths }}",
      self.get_subpaths().map(|s| s.len()).unwrap_or(0)
    )
  }
}

impl SubpathsSeq {
  fn get_tracer(&self) -> Result<&PathTracerCallable, ErrorStack> {
    if let Callable::Dynamic { inner, .. } = self.tracer.as_ref() {
      if let Some(tracer) = inner.as_any().downcast_ref::<PathTracerCallable>() {
        return Ok(tracer);
      }
    }
    Err(ErrorStack::new(
      "Internal error: `SubpathsSeq` constructed with non-`PathTracerCallable` tracer",
    ))
  }

  fn get_subpaths(&self) -> Result<&[PathSubpath], ErrorStack> {
    let tracer = self.get_tracer()?;
    Ok(&tracer.subpaths)
  }

  pub fn new(tracer: Rc<Callable>) -> Self {
    Self { tracer }
  }
}

impl Sequence for SubpathsSeq {
  fn consume<'a>(
    &self,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let tracer = match self.get_tracer() {
      Ok(t) => t,
      Err(e) => {
        return Box::new(std::iter::once(Err(e)));
      }
    };
    let interned_t_kwarg = tracer.interned_t_kwarg;
    let reverse = tracer.reverse;
    let subpaths = tracer.subpaths.clone();

    Box::new(subpaths.into_iter().map(move |subpath| {
      let tracer = PathTracerCallable::from_subpath(subpath, interned_t_kwarg, reverse);
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "trace_path".to_owned(),
        inner: Box::new(tracer),
      })))
    }))
  }
}

fn angle_between(a: Vec2, b: Vec2) -> f32 {
  let la = a.norm();
  let lb = b.norm();
  if la <= LENGTH_EPSILON || lb <= LENGTH_EPSILON {
    return 0.0;
  }
  let cos = (a.dot(&b) / (la * lb)).clamp(-1.0, 1.0);
  cos.acos()
}

fn segment_turning_angle(seg: &PathSegment) -> f32 {
  match seg {
    PathSegment::Line { .. } => 0.0,
    PathSegment::Quadratic {
      start, ctrl, end, ..
    } => angle_between(*ctrl - *start, *end - *ctrl),
    PathSegment::Cubic {
      start,
      ctrl1,
      ctrl2,
      end,
      ..
    } => {
      let a = angle_between(*ctrl1 - *start, *ctrl2 - *ctrl1);
      let b = angle_between(*ctrl2 - *ctrl1, *end - *ctrl2);
      a + b
    }
    PathSegment::Arc { theta_delta, .. } => theta_delta.abs(),
  }
}

fn segment_subdivisions(seg: &PathSegment, angle_tolerance: f32) -> usize {
  if angle_tolerance <= 0.0 || !seg.has_detail() {
    return 1;
  }
  let angle = segment_turning_angle(seg);
  if angle <= 0.0 {
    return 1;
  }
  let count = (angle / angle_tolerance).ceil() as usize;
  count.max(1)
}

pub(crate) fn sample_subpath_points(
  subpath: &PathSubpath,
  angle_tolerance: f32,
  include_end: bool,
) -> Vec<Vec2> {
  if subpath.segments.is_empty() || subpath.total_length <= LENGTH_EPSILON {
    return Vec::new();
  }

  let mut extra = 0usize;
  for seg in &subpath.segments {
    extra = extra.saturating_add(segment_subdivisions(seg, angle_tolerance).saturating_sub(1));
  }

  let base_count = if include_end {
    subpath.segments.len() + 1
  } else {
    subpath.segments.len()
  };
  let target_count = base_count.saturating_add(extra);

  let guides = subpath.critical_t_values();
  let intervals = subpath.segment_intervals();
  let interval_weights = build_interval_weights(&guides, &[intervals]);
  let t_samples = build_topology_samples(
    target_count,
    Some(&guides),
    interval_weights.as_deref(),
    include_end,
  );

  t_samples
    .into_iter()
    .map(|t| subpath.sample_by_length(t * subpath.total_length()))
    .collect()
}

impl DynamicCallable for PathTracerCallable {
  fn as_any(&self) -> &dyn Any {
    self
  }

  fn invoke(
    &self,
    args: &[crate::Value],
    kwargs: &FxHashMap<Sym, Value>,
    _ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let t = if !kwargs.is_empty() {
      if kwargs.len() != 1 || !kwargs.contains_key(&self.interned_t_kwarg) {
        return Err(ErrorStack::new(
          "Unexpected keyword arguments; expected only `t`",
        ));
      }
      if !args.is_empty() {
        return Err(ErrorStack::new(
          "Expected only keyword argument `t` and no positional args",
        ));
      }
      kwargs.get(&self.interned_t_kwarg).unwrap()
    } else {
      if args.len() != 1 {
        return Err(ErrorStack::new("Expected argument `t`"));
      }
      &args[0]
    };
    let Some(t) = t.as_float() else {
      return Err(ErrorStack::new(format!(
        "Expected 't' to be a number, found {t:?}"
      )));
    };
    let t = t.clamp(0., 1.);

    let pos = self.sample(t)?;
    Ok(Value::Vec2(pos))
  }

  fn get_return_type_hint(&self) -> Option<ArgType> {
    Some(ArgType::Vec2)
  }

  fn is_side_effectful(&self) -> bool {
    false
  }

  fn is_rng_dependent(&self) -> bool {
    false
  }
}

#[derive(Clone, Debug)]
pub enum DrawCommand {
  MoveTo(Vec2),
  LineTo(Vec2),
  QuadraticBezier {
    ctrl: Vec2,
    to: Vec2,
  },
  SmoothQuadraticBezier {
    to: Vec2,
  },
  CubicBezier {
    ctrl1: Vec2,
    ctrl2: Vec2,
    to: Vec2,
  },
  SmoothCubicBezier {
    ctrl2: Vec2,
    to: Vec2,
  },
  Arc {
    rx: f32,
    ry: f32,
    x_axis_rotation: f32,
    large_arc: bool,
    sweep: bool,
    to: Vec2,
  },
  Close,
}

struct DrawCtx {
  pub cmds: AppendOnlyBuffer<DrawCommand>,
}

impl Default for DrawCtx {
  fn default() -> Self {
    Self {
      cmds: AppendOnlyBuffer::default(),
    }
  }
}

impl DrawCtx {
  fn into_inner(&self) -> Vec<DrawCommand> {
    // there might be references to this floating around, and who cares about a clone here anyway
    self.cmds.borrow().to_vec()
  }
}

fn inject_draw_commands(ctx: &EvalCtx, scope: &Scope, draw_ctx: &Rc<DrawCtx>) {
  fn draw_command_kind_for_name(name: &str) -> Option<DrawCommandKind> {
    match name {
      "move" => Some(DrawCommandKind::Move),
      "line" => Some(DrawCommandKind::Line),
      "quadratic_bezier" => Some(DrawCommandKind::Quadratic),
      "smooth_quadratic_bezier" => Some(DrawCommandKind::SmoothQuadratic),
      "cubic_bezier" => Some(DrawCommandKind::Cubic),
      "smooth_cubic_bezier" => Some(DrawCommandKind::SmoothCubic),
      "arc" => Some(DrawCommandKind::Arc),
      "close" => Some(DrawCommandKind::Close),
      _ => None,
    }
  }

  fn insert_cmd(
    ctx: &EvalCtx,
    scope: &Scope,
    draw_ctx: &Rc<DrawCtx>,
    name: &'static str,
    kind: DrawCommandKind,
  ) {
    scope.insert(
      ctx.interned_symbols.intern(name),
      Value::Callable(Rc::new(Callable::Dynamic {
        name: format!("trace_path.{name}"),
        inner: Box::new(DrawCommandCallable {
          fn_name: name,
          kind,
          draw_ctx: Rc::clone(draw_ctx),
        }),
      })),
    );
  }

  let canonical = [
    "move",
    "line",
    "quadratic_bezier",
    "smooth_quadratic_bezier",
    "cubic_bezier",
    "smooth_cubic_bezier",
    "arc",
    "close",
  ];
  for name in canonical {
    if let Some(kind) = draw_command_kind_for_name(name) {
      insert_cmd(ctx, scope, draw_ctx, name, kind);
    }
  }

  // Trace-path-specific alias; global aliasing maps "bezier" to 3d.
  insert_cmd(ctx, scope, draw_ctx, "bezier", DrawCommandKind::Cubic);

  for (alias, target) in FUNCTION_ALIASES.entries() {
    if let Some(kind) = draw_command_kind_for_name(target) {
      insert_cmd(ctx, scope, draw_ctx, alias, kind);
    }
  }
}

#[derive(Clone, Copy)]
enum DrawCommandKind {
  Move,
  Line,
  Quadratic,
  SmoothQuadratic,
  Cubic,
  SmoothCubic,
  Arc,
  Close,
}

struct DrawCommandCallable {
  fn_name: &'static str,
  kind: DrawCommandKind,
  draw_ctx: Rc<DrawCtx>,
}

impl DrawCommandCallable {
  fn fn_name(&self) -> &'static str {
    self.fn_name
  }
}

impl DynamicCallable for DrawCommandCallable {
  fn as_any(&self) -> &dyn Any {
    self
  }

  fn invoke(
    &self,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let fn_name = self.fn_name();
    let resolved_name = match fn_name {
      "quad_bezier" => "quadratic_bezier",
      "smooth_quad_bezier" => "smooth_quadratic_bezier",
      "smooth_bezier" => "smooth_cubic_bezier",
      "bezier" => "cubic_bezier",
      _ => fn_name,
    };
    let fn_def = fn_sigs()
      .get(resolved_name)
      .ok_or_else(|| ErrorStack::new(format!("Unknown draw command `{fn_name}`")))?;
    let (def_ix, arg_refs) = match get_args(ctx, fn_name, fn_def.signatures, args, kwargs)? {
      GetArgsOutput::Valid { def_ix, arg_refs } => (def_ix, arg_refs),
      GetArgsOutput::PartiallyApplied => {
        return Err(ErrorStack::new(format!(
          "Draw commands do not support partial application.\n\nAvailable signatures for \
           `{fn_name}`:\n{}",
          format_fn_signatures(fn_def.signatures)
        )));
      }
    };

    match self.kind {
      DrawCommandKind::Move => {
        let pos = match def_ix {
          0 => {
            let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            Vec2::new(x, y)
          }
          1 => *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
          _ => unreachable!(),
        };
        self.draw_ctx.cmds.push(DrawCommand::MoveTo(pos));
      }
      DrawCommandKind::Line => {
        let pos = match def_ix {
          0 => {
            let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            Vec2::new(x, y)
          }
          1 => *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
          _ => unreachable!(),
        };
        self.draw_ctx.cmds.push(DrawCommand::LineTo(pos));
      }
      DrawCommandKind::Quadratic => {
        let (ctrl, to) = match def_ix {
          0 => (
            *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[1].resolve(args, kwargs).as_vec2().unwrap(),
          ),
          1 => {
            let cx = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let cy = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            let x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            (Vec2::new(cx, cy), Vec2::new(x, y))
          }
          _ => unreachable!(),
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::QuadraticBezier { ctrl, to });
      }
      DrawCommandKind::SmoothQuadratic => {
        let to = match def_ix {
          0 => *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
          1 => {
            let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            Vec2::new(x, y)
          }
          _ => unreachable!(),
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::SmoothQuadraticBezier { to });
      }
      DrawCommandKind::Cubic => {
        let (ctrl1, ctrl2, to) = match def_ix {
          0 => (
            *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[1].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[2].resolve(args, kwargs).as_vec2().unwrap(),
          ),
          1 => {
            let c1x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let c1y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            let c2x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
            let c2y = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            let x = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
            (Vec2::new(c1x, c1y), Vec2::new(c2x, c2y), Vec2::new(x, y))
          }
          _ => {
            return Err(ErrorStack::new(format!(
              "`{fn_name}` cannot be used with Vec3 inputs inside `trace_path`; use `bezier3d` \
               outside of `trace_path` or `cubic_bezier` with Vec2 values"
            )))
          }
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::CubicBezier { ctrl1, ctrl2, to });
      }
      DrawCommandKind::SmoothCubic => {
        let (ctrl2, to) = match def_ix {
          0 => (
            *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[1].resolve(args, kwargs).as_vec2().unwrap(),
          ),
          1 => {
            let c2x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let c2y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            let x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            (Vec2::new(c2x, c2y), Vec2::new(x, y))
          }
          _ => unreachable!(),
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::SmoothCubicBezier { ctrl2, to });
      }
      DrawCommandKind::Arc => {
        let rx = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
        let ry = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
        let x_axis_rotation = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
        let (large_arc, sweep, to) = match def_ix {
          0 => {
            let large_arc = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
            let sweep = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();
            let x = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[6].resolve(args, kwargs).as_float().unwrap();
            (large_arc, sweep, Vec2::new(x, y))
          }
          1 => {
            let large_arc = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
            let sweep = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();
            let to = *arg_refs[5].resolve(args, kwargs).as_vec2().unwrap();
            (large_arc, sweep, to)
          }
          2 => {
            let x = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
            (false, true, Vec2::new(x, y))
          }
          3 => {
            let to = *arg_refs[3].resolve(args, kwargs).as_vec2().unwrap();
            (false, true, to)
          }
          _ => unreachable!(),
        };
        self.draw_ctx.cmds.push(DrawCommand::Arc {
          rx,
          ry,
          x_axis_rotation,
          large_arc,
          sweep,
          to,
        });
      }
      DrawCommandKind::Close => {
        self.draw_ctx.cmds.push(DrawCommand::Close);
      }
    }

    Ok(Value::Nil)
  }

  fn get_return_type_hint(&self) -> Option<ArgType> {
    Some(ArgType::Nil)
  }

  fn is_side_effectful(&self) -> bool {
    true
  }

  fn is_rng_dependent(&self) -> bool {
    false
  }
}

fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, t: f32) -> Vec2 {
  let u = 1.0 - t;
  let tt = t * t;
  let uu = u * u;
  uu * p0 + 2.0 * u * t * p1 + tt * p2
}

fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
  let u = 1.0 - t;
  let tt = t * t;
  let uu = u * u;
  let uuu = uu * u;
  let ttt = tt * t;
  uuu * p0 + 3.0 * uu * t * p1 + 3.0 * u * tt * p2 + ttt * p3
}

fn arc_point(
  center: Vec2,
  rx: f32,
  ry: f32,
  cos_phi: f32,
  sin_phi: f32,
  theta_start: f32,
  theta_delta: f32,
  t: f32,
) -> Vec2 {
  let theta = theta_start + theta_delta * t;
  let (sin_theta, cos_theta) = theta.sin_cos();
  let x = rx * cos_theta;
  let y = ry * sin_theta;
  let px = cos_phi * x - sin_phi * y + center.x;
  let py = sin_phi * x + cos_phi * y + center.y;
  Vec2::new(px, py)
}

fn build_arc_segment(
  start: Vec2,
  end: Vec2,
  rx: f32,
  ry: f32,
  x_axis_rotation: f32,
  large_arc: bool,
  sweep: bool,
) -> Option<(PathSegment, Vec2, Vec2)> {
  let mut rx = rx.abs();
  let mut ry = ry.abs();
  if rx <= LENGTH_EPSILON || ry <= LENGTH_EPSILON {
    let length = (end - start).norm();
    let mut min = start;
    let mut max = start;
    extend_bounds(&mut min, &mut max, end);
    return Some((PathSegment::Line { start, end, length }, min, max));
  }

  if (end - start).norm() <= LENGTH_EPSILON {
    return None;
  }

  let phi = x_axis_rotation.to_radians();
  let cos_phi = phi.cos();
  let sin_phi = phi.sin();
  let dx = (start.x - end.x) / 2.0;
  let dy = (start.y - end.y) / 2.0;
  let x1p = cos_phi * dx + sin_phi * dy;
  let y1p = -sin_phi * dx + cos_phi * dy;

  let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
  if lambda > 1.0 {
    let scale = lambda.sqrt();
    rx *= scale;
    ry *= scale;
  }

  let rx_sq = rx * rx;
  let ry_sq = ry * ry;
  let x1p_sq = x1p * x1p;
  let y1p_sq = y1p * y1p;
  let denom = rx_sq * y1p_sq + ry_sq * x1p_sq;
  if denom.abs() <= LENGTH_EPSILON {
    let length = (end - start).norm();
    let mut min = start;
    let mut max = start;
    extend_bounds(&mut min, &mut max, end);
    return Some((PathSegment::Line { start, end, length }, min, max));
  }

  let numerator = rx_sq * ry_sq - rx_sq * y1p_sq - ry_sq * x1p_sq;
  let coef = (numerator / denom).max(0.).sqrt();
  let sign = if large_arc == sweep { -1. } else { 1. };
  let coef = sign * coef;

  let cxp = coef * (rx * y1p / ry);
  let cyp = coef * (-ry * x1p / rx);
  let cx = cos_phi * cxp - sin_phi * cyp + (start.x + end.x) / 2.;
  let cy = sin_phi * cxp + cos_phi * cyp + (start.y + end.y) / 2.;
  let center = Vec2::new(cx, cy);

  let v1 = Vec2::new((x1p - cxp) / rx, (y1p - cyp) / ry);
  let v2 = Vec2::new((-x1p - cxp) / rx, (-y1p - cyp) / ry);
  let theta_start = v1.y.atan2(v1.x);
  let mut theta_delta = (v1.x * v2.y - v1.y * v2.x).atan2(v1.x * v2.x + v1.y * v2.y);

  if !sweep && theta_delta > 0. {
    theta_delta -= 2. * PI;
  } else if sweep && theta_delta < 0. {
    theta_delta += 2. * PI;
  }

  let (table, min, max) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
    arc_point(
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      t,
    )
  });

  Some((
    PathSegment::Arc {
      end,
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      table,
    },
    min,
    max,
  ))
}

pub(crate) const TRACE_PATH_DRAW_COMMAND_NAMES: [&str; 8] = [
  "move",
  "line",
  "quadratic_bezier",
  "smooth_quadratic_bezier",
  "cubic_bezier",
  "smooth_cubic_bezier",
  "arc",
  "close",
];

fn eval_trace_path_cb(ctx: &EvalCtx, cb: &Callable) -> Result<Vec<DrawCommand>, ErrorStack> {
  let Callable::Closure(closure) = cb else {
    return Err(ErrorStack::new(
      "You must pass a closure directly to `trace_path`'s callback argument.  The closure's scope \
       is specially modified to make the path drawing commands available.",
    ));
  };

  let captured_scope = match &closure.captured_scope {
    CapturedScope::Strong(scope) => Rc::clone(&scope),
    CapturedScope::Weak(weak) => {
      log::error!("I'm pretty sure this isn't possible except in recursive call cases...");
      weak.upgrade().ok_or_else(|| {
        ErrorStack::new("Internal error: captured scope has been dropped unexpectedly")
      })?
    }
  };

  let wrapped_scope = Scope::wrap(captured_scope);

  let draw_ctx = Rc::new(DrawCtx::default());
  inject_draw_commands(ctx, &wrapped_scope, &draw_ctx);

  let mut closure: Closure = closure.clone();

  // Const folding will also work against us by inserting builtin callable literals mapping to the
  // placeholder draw command stubs that just error out.
  //
  // We have to traverse the closure body and replace them with the actual draw command callables.
  let mut body: ClosureBody = (*closure.body).clone();

  let mut draw_cmd_name_by_entry_ix = FxHashMap::default();
  for name in TRACE_PATH_DRAW_COMMAND_NAMES {
    let entry_ix = get_builtin_fn_sig_entry_ix(name).unwrap();
    draw_cmd_name_by_entry_ix.insert(entry_ix, name);
  }
  let mut traverse = |expr: &mut Expr| {
    fn traverse_inner(
      ctx: &EvalCtx,
      draw_cmd_name_by_entry_ix: &FxHashMap<usize, &str>,
      expr: &mut Expr,
    ) {
      match expr {
        Expr::Call {
          call: FunctionCall { target, .. },
          ..
        } => match target {
          FunctionCallTarget::Literal(callable) => match &**callable {
            Callable::Builtin { fn_entry_ix, .. } => {
              dbg!(fn_sigs().entries[*fn_entry_ix].0);
              if let Some(name) = draw_cmd_name_by_entry_ix.get(fn_entry_ix) {
                *target = FunctionCallTarget::Name(ctx.interned_symbols.intern(name));
              }
            }
            _ => (),
          },
          _ => (),
        },
        // users can define helper functions inside the closure that also use draw commands
        Expr::Closure {
          body: inner_body, ..
        } => {
          let mut new_helper_body: ClosureBody = (**inner_body).clone();
          let mut traverse_helper = |expr: &mut Expr| {
            traverse_inner(ctx, draw_cmd_name_by_entry_ix, expr);
          };
          new_helper_body.traverse_exprs_mut(&mut traverse_helper);
          *inner_body = Rc::new(new_helper_body);
        }
        _ => (),
      }
    }

    traverse_inner(ctx, &draw_cmd_name_by_entry_ix, expr);
  };
  body.traverse_exprs_mut(&mut traverse);
  closure.body = Rc::new(body);

  closure.captured_scope = CapturedScope::Strong(Rc::new(wrapped_scope));
  ctx
    .invoke_closure(&closure, EMPTY_ARGS, EMPTY_KWARGS)
    .map_err(|err| err.wrap("Error while executing user-provided path tracing callback"))?;

  Ok(draw_ctx.into_inner())
}

pub(crate) fn draw_command_stub_impl(
  name: &'static str,
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
  _ctx: &EvalCtx,
) -> Result<Value, ErrorStack> {
  Err(ErrorStack::new(format!(
    "`{name}` can only be called within the callback passed to `trace_path`",
  )))
}

pub fn trace_path_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let closed = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
      let center = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
      let reverse = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();

      let draw_cmds = eval_trace_path_cb(ctx, cb)
        .map_err(|err| err.wrap("Error while evaluating callback provided to `trace_path`"))?;

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let path_tracer =
        PathTracerCallable::new(closed, center, reverse, draw_cmds, interned_t_kwarg);
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "trace_path".to_string(),
        inner: Box::new(path_tracer),
      })))
    }
    _ => unimplemented!(),
  }
}

fn parse_svg_path_to_draw_commands(svg_path_str: &str) -> Result<Vec<DrawCommand>, ErrorStack> {
  let parser = PathParser::from(svg_path_str);

  let mut draw_cmds = Vec::new();
  let mut current_pos = Vec2::new(0.0, 0.0);
  let mut start_pos = Vec2::new(0.0, 0.0); // For ClosePath

  for segment in parser {
    let segment =
      segment.map_err(|err| ErrorStack::new(format!("invalid SVG path data: {err}",)))?;
    match segment {
      svgtypes::PathSegment::MoveTo { abs, x, y } => {
        let pos = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::MoveTo(pos));
        current_pos = pos;
        start_pos = pos;
      }
      svgtypes::PathSegment::LineTo { abs, x, y } => {
        let pos = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::LineTo(pos));
        current_pos = pos;
      }
      svgtypes::PathSegment::HorizontalLineTo { abs, x } => {
        let pos = if abs {
          Vec2::new(x as f32, current_pos.y)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y)
        };
        draw_cmds.push(DrawCommand::LineTo(pos));
        current_pos = pos;
      }
      svgtypes::PathSegment::VerticalLineTo { abs, y } => {
        let pos = if abs {
          Vec2::new(current_pos.x, y as f32)
        } else {
          Vec2::new(current_pos.x, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::LineTo(pos));
        current_pos = pos;
      }
      svgtypes::PathSegment::CurveTo {
        abs,
        x1,
        y1,
        x2,
        y2,
        x,
        y,
      } => {
        let (ctrl1, ctrl2, to) = if abs {
          (
            Vec2::new(x1 as f32, y1 as f32),
            Vec2::new(x2 as f32, y2 as f32),
            Vec2::new(x as f32, y as f32),
          )
        } else {
          (
            Vec2::new(current_pos.x + x1 as f32, current_pos.y + y1 as f32),
            Vec2::new(current_pos.x + x2 as f32, current_pos.y + y2 as f32),
            Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32),
          )
        };
        draw_cmds.push(DrawCommand::CubicBezier { ctrl1, ctrl2, to });
        current_pos = to;
      }
      svgtypes::PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
        let (ctrl2, to) = if abs {
          (
            Vec2::new(x2 as f32, y2 as f32),
            Vec2::new(x as f32, y as f32),
          )
        } else {
          (
            Vec2::new(current_pos.x + x2 as f32, current_pos.y + y2 as f32),
            Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32),
          )
        };
        draw_cmds.push(DrawCommand::SmoothCubicBezier { ctrl2, to });
        current_pos = to;
      }
      svgtypes::PathSegment::Quadratic { abs, x1, y1, x, y } => {
        let (ctrl, to) = if abs {
          (
            Vec2::new(x1 as f32, y1 as f32),
            Vec2::new(x as f32, y as f32),
          )
        } else {
          (
            Vec2::new(current_pos.x + x1 as f32, current_pos.y + y1 as f32),
            Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32),
          )
        };
        draw_cmds.push(DrawCommand::QuadraticBezier { ctrl, to });
        current_pos = to;
      }
      svgtypes::PathSegment::SmoothQuadratic { abs, x, y } => {
        let to = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::SmoothQuadraticBezier { to });
        current_pos = to;
      }
      svgtypes::PathSegment::EllipticalArc {
        abs,
        rx,
        ry,
        x_axis_rotation,
        large_arc,
        sweep,
        x,
        y,
      } => {
        let to = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::Arc {
          rx: rx as f32,
          ry: ry as f32,
          x_axis_rotation: x_axis_rotation as f32,
          large_arc,
          sweep,
          to,
        });
        current_pos = to;
      }
      svgtypes::PathSegment::ClosePath { abs: _ } => {
        draw_cmds.push(DrawCommand::Close);
        current_pos = start_pos;
      }
    }
  }

  Ok(draw_cmds)
}

pub fn trace_svg_path_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let svg_path_str = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
      let center = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
      let reverse = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();

      let draw_cmds = parse_svg_path_to_draw_commands(svg_path_str)
        .map_err(|err| err.wrap("Error while parsing SVG path string"))?;

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let path_tracer =
        PathTracerCallable::new(false, center, reverse, draw_cmds, interned_t_kwarg);
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "trace_svg_path".to_string(),
        inner: Box::new(path_tracer),
      })))
    }
    _ => unimplemented!(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parse_and_eval_program;

  fn assert_vec2_close(actual: Vec2, expected: Vec2) {
    let diff = (actual - expected).norm();
    assert!(
      diff < 1e-4,
      "Expected {expected:?}, got {actual:?} (diff {diff})"
    );
  }

  #[test]
  fn test_path_tracer_line_segments() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 3.0)),
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.25).unwrap(), Vec2::new(1.0, 0.0));
    assert_vec2_close(tracer.sample(0.75).unwrap(), Vec2::new(1.0, 2.0));
  }

  #[test]
  fn test_path_tracer_critical_t_values() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 1.0)),
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let guides = tracer.critical_t_values();

    assert_eq!(guides.len(), 3);
    assert!((guides[0] - 0.0).abs() < 1e-6);
    assert!((guides[1] - 0.5).abs() < 1e-6);
    assert!((guides[2] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_path_tracer_override_critical_points() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 1.0)),
      DrawCommand::LineTo(Vec2::new(0.0, 1.0)),
      DrawCommand::Close,
    ];
    let override_cps = vec![0.0, 0.25, 0.5, 0.75, 1.0];
    let tracer = PathTracerCallable::new_with_critical_points(
      false,
      false,
      false,
      cmds,
      Sym(0),
      Some(override_cps.clone()),
    );

    assert_eq!(tracer.critical_t_values(), override_cps);
  }

  #[test]
  fn test_path_tracer_multiple_subpaths_sampling() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::MoveTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(11.0, 0.0)),
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.25).unwrap(), Vec2::new(0.5, 0.0));
    assert_vec2_close(tracer.sample(0.75).unwrap(), Vec2::new(10.5, 0.0));
  }

  #[test]
  fn test_path_tracer_subpath_close_uses_local_start() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::Close,
      DrawCommand::MoveTo(Vec2::new(5.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(6.0, 0.0)),
      DrawCommand::Close,
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_eq!(tracer.subpaths.len(), 2);
    assert_eq!(tracer.subpaths[1].segments.len(), 2);
    match &tracer.subpaths[1].segments[1] {
      PathSegment::Line { start, end, .. } => {
        assert_vec2_close(*start, Vec2::new(6.0, 0.0));
        assert_vec2_close(*end, Vec2::new(5.0, 0.0));
      }
      _ => panic!("Expected closing line segment for subpath close"),
    }
  }

  #[test]
  fn test_sample_subpath_points_curvature_detail() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::QuadraticBezier {
        ctrl: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let subpath = &tracer.subpaths[0];

    let points = sample_subpath_points(subpath, std::f32::consts::FRAC_PI_4, true);
    assert_eq!(points.len(), 3);
    assert_vec2_close(points[0], Vec2::new(0.0, 0.0));
    assert_vec2_close(points[2], Vec2::new(2.0, 0.0));
  }

  #[test]
  fn test_build_topology_samples_include_end() {
    let samples = build_topology_samples(3, None, None, true);
    assert_eq!(samples.len(), 3);
    assert!((samples[0] - 0.0).abs() < 1e-6);
    assert!((samples[1] - 0.5).abs() < 1e-6);
    assert!((samples[2] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_path_tracer_segment_intervals_detail_order() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::QuadraticBezier {
        ctrl: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds.clone(), Sym(0));
    let intervals = tracer.segment_intervals();

    assert_eq!(intervals.len(), 2);
    assert!(!intervals[0].has_detail);
    assert!(intervals[1].has_detail);

    let reverse_tracer = PathTracerCallable::new(false, false, true, cmds, Sym(0));
    let reverse_intervals = reverse_tracer.segment_intervals();

    assert_eq!(reverse_intervals.len(), 2);
    assert!(reverse_intervals[0].has_detail);
    assert!(!reverse_intervals[1].has_detail);
  }

  #[test]
  fn test_path_tracer_centering() {
    // 10x10 Box ending at (10, 10). Center is (5, 5).
    // Result should be shifted by (-5, -5), moving (0,0) to (-5, -5).
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 10.0)),
    ];
    let tracer = PathTracerCallable::new(false, true, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(-5.0, -5.0)); // was 0,0
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(5.0, -5.0)); // was 10,0
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(5.0, 5.0)); // was 10,10
  }

  #[test]
  fn test_path_tracer_quadratic_endpoints() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::QuadraticBezier {
        ctrl: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(2.0, 0.0));
  }

  #[test]
  fn test_path_tracer_smooth_cubic_reflection() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::CubicBezier {
        ctrl1: Vec2::new(0.0, 1.0),
        ctrl2: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
      DrawCommand::SmoothCubicBezier {
        ctrl2: Vec2::new(4.0, 2.0),
        to: Vec2::new(5.0, 0.0),
      },
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_eq!(tracer.subpaths.len(), 1);
    assert_eq!(tracer.subpaths[0].segments.len(), 2);
    match &tracer.subpaths[0].segments[1] {
      PathSegment::Cubic { ctrl1, .. } => {
        assert_vec2_close(*ctrl1, Vec2::new(3.0, -1.0));
      }
      _ => panic!("Expected cubic segment for smooth cubic reflection"),
    }
  }

  #[test]
  fn test_path_tracer_smooth_quadratic_reflection() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::QuadraticBezier {
        ctrl: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
      DrawCommand::SmoothQuadraticBezier {
        to: Vec2::new(4.0, 0.0),
      },
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_eq!(tracer.subpaths.len(), 1);
    assert_eq!(tracer.subpaths[0].segments.len(), 2);
    match &tracer.subpaths[0].segments[1] {
      PathSegment::Quadratic { ctrl, .. } => {
        assert_vec2_close(*ctrl, Vec2::new(3.0, -1.0));
      }
      _ => panic!("Expected quadratic segment for smooth quadratic reflection"),
    }
  }

  #[test]
  fn test_path_tracer_arc_endpoints() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(1.0, 0.0)),
      DrawCommand::Arc {
        rx: 1.0,
        ry: 1.0,
        x_axis_rotation: 0.0,
        large_arc: false,
        sweep: true,
        to: Vec2::new(-1.0, 0.0),
      },
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(1.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(-1.0, 0.0));
  }

  #[test]
  fn test_path_tracer_closed_flag_adds_closing_segment() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 2.0)),
    ];
    let tracer = PathTracerCallable::new(true, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(0.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_absolute_line() {
    // Simple absolute path: move to origin, line to (10, 0), line to (10, 10)
    let svg = "M 0 0 L 10 0 L 10 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(10.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_relative_line() {
    // Relative path: move to (5, 5), relative line +10 in x, then +10 in y
    let svg = "M 5 5 l 10 0 l 0 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(5.0, 5.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(15.0, 5.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(15.0, 15.0));
  }

  #[test]
  fn test_parse_svg_path_horizontal_vertical() {
    // H and V commands
    let svg = "M 0 0 H 10 V 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(10.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_cubic_bezier() {
    // Cubic bezier from (0,0) to (10,0) with control points
    let svg = "M 0 0 C 3 5, 7 5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_quadratic_bezier() {
    // Quadratic bezier from (0,0) to (10,0) with control point at (5, 5)
    let svg = "M 0 0 Q 5 5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_arc() {
    // Arc from (1,0) to (-1,0) with rx=ry=1
    let svg = "M 1 0 A 1 1 0 0 1 -1 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(1.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(-1.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_close() {
    // Triangle that closes back to start
    let svg = "M 0 0 L 10 0 L 5 10 Z";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();

    // Should have MoveTo, LineTo, LineTo, Close
    assert_eq!(cmds.len(), 4);
    assert!(matches!(cmds[3], DrawCommand::Close));

    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    // With close, path goes back to origin
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(0.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_smooth_cubic() {
    // Smooth cubic: S command reflects the previous control point
    let svg = "M 0 0 C 0 5, 5 5, 5 0 S 10 -5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    assert!(matches!(cmds[2], DrawCommand::SmoothCubicBezier { .. }));
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert!(matches!(
      tracer.subpaths[0].segments[1],
      PathSegment::Cubic { .. }
    ));
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_smooth_quadratic() {
    // Smooth quadratic: T command reflects the previous control point
    let svg = "M 0 0 Q 2.5 5, 5 0 T 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    assert!(matches!(cmds[2], DrawCommand::SmoothQuadraticBezier { .. }));
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert!(matches!(
      tracer.subpaths[0].segments[1],
      PathSegment::Quadratic { .. }
    ));
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_trace_path_alias_draw_commands() {
    let src = r#"
path = trace_path(|| {
  move(0, 0)
  quad_bezier(vec2(1, 0), vec2(2, 0))
  smooth_quadratic_bezier(3, 0)
  cubic_bezier(vec2(4, 0), vec2(5, 0), vec2(6, 0))
  smooth_bezier(vec2(7, 0), vec2(8, 0))
})
p0 = path(0)
p1 = path(1)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    let p0_val = ctx.get_global("p0").unwrap();
    let p1_val = ctx.get_global("p1").unwrap();
    let p0 = p0_val.as_vec2().unwrap();
    let p1 = p1_val.as_vec2().unwrap();

    assert_vec2_close(*p0, Vec2::new(0.0, 0.0));
    assert_vec2_close(*p1, Vec2::new(8.0, 0.0));
  }

  #[test]
  fn test_tessellate_path_from_sequence() {
    let src = r#"
path = [vec2(0, 0), vec2(1, 0), vec2(0, 1)]
mesh = tessellate_path(path)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh = mesh_val.as_mesh().unwrap();
    assert_eq!(mesh.mesh.faces.len(), 1);
  }

  #[test]
  fn test_tessellate_path_from_trace_path() {
    let src = r#"
path = trace_path(|| {
  move(0, 0)
  line(1, 0)
  line(1, 1)
  line(0, 1)
  close()
})
mesh = tessellate_path(path)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh = mesh_val.as_mesh().unwrap();
    assert_eq!(mesh.mesh.vertices.len(), 4);
    assert_eq!(mesh.mesh.faces.len(), 2);
  }

  #[test]
  fn test_subpaths_builtin() {
    let src = r#"
// Create a path with two disconnected subpaths via two move commands
path = trace_path(|| {
  move(0, 0)
  line(10, 0)

  move(100, 0)
  line(100, 20)
})

// Extract the subpaths as a sequence
subs = subpaths(path)

// Collect the subpaths into an array and sample them
sub_array = collect(subs)
count = len(sub_array)

// Sample the first subpath
first = sub_array[0]
first_start = first(0)
first_end = first(1)

// Sample the second subpath
second = sub_array[1]
second_start = second(0)
second_end = second(1)
"#;

    let ctx = parse_and_eval_program(src).unwrap();

    // Verify we have 2 subpaths
    let count = ctx.get_global("count").unwrap();
    assert_eq!(count.as_int().unwrap(), 2);

    // Verify first subpath samples correctly
    let first_start = ctx.get_global("first_start").unwrap();
    let first_end = ctx.get_global("first_end").unwrap();
    assert_vec2_close(*first_start.as_vec2().unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(*first_end.as_vec2().unwrap(), Vec2::new(10.0, 0.0));

    // Verify second subpath samples correctly
    let second_start = ctx.get_global("second_start").unwrap();
    let second_end = ctx.get_global("second_end").unwrap();
    assert_vec2_close(*second_start.as_vec2().unwrap(), Vec2::new(100.0, 0.0));
    assert_vec2_close(*second_end.as_vec2().unwrap(), Vec2::new(100.0, 20.0));
  }
}
