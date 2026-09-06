//! Green's-theorem area/first-moment integrals per segment, plus arc-length moments.

use std::f64::consts::FRAC_PI_2;

use super::segment::*;
use crate::Vec2;

const GL_X: [f64; 8] = [
  -0.9602898564975363,
  -0.7966664774136267,
  -0.5255324099163290,
  -0.1834346424956498,
  0.1834346424956498,
  0.5255324099163290,
  0.7966664774136267,
  0.9602898564975363,
];
const GL_W: [f64; 8] = [
  0.1012285362903763,
  0.2223810344533745,
  0.3137066458778873,
  0.3626837833783620,
  0.3626837833783620,
  0.3137066458778873,
  0.2223810344533745,
  0.1012285362903763,
];

/// `(∮x dy, ∮x² dy, ∮y² dx)` along the segment; `f(t)` yields `(x, y, dx/dt, dy/dt)`.
fn gauss_moments(t0: f64, t1: f64, f: impl Fn(f64) -> (f64, f64, f64, f64)) -> (f64, f64, f64) {
  let (half, mid) = ((t1 - t0) * 0.5, (t1 + t0) * 0.5);
  let mut acc = (0., 0., 0.);
  for (x, w) in GL_X.iter().zip(GL_W) {
    let (px, py, dx, dy) = f(mid + half * x);
    let w = w * half;
    acc.0 += w * px * dy;
    acc.1 += w * px * px * dy;
    acc.2 += w * py * py * dx;
  }
  acc
}

fn v(p: &Vec2) -> (f64, f64) {
  (p.x as f64, p.y as f64)
}

pub(crate) fn segment_moments(seg: &PathSegment) -> (f64, f64, f64) {
  match seg {
    PathSegment::Line { start, end, .. } => {
      let ((x0, y0), (x1, y1)) = (v(start), v(end));
      let (dx, dy) = (x1 - x0, y1 - y0);
      (
        dy * (x0 + x1) / 2.,
        dy * (x0 * x0 + x0 * x1 + x1 * x1) / 3.,
        dx * (y0 * y0 + y0 * y1 + y1 * y1) / 3.,
      )
    }
    PathSegment::Quadratic {
      start, ctrl, end, ..
    } => {
      let (p0, p1, p2) = (v(start), v(ctrl), v(end));
      gauss_moments(0., 1., |t| {
        let u = 1. - t;
        (
          u * u * p0.0 + 2. * u * t * p1.0 + t * t * p2.0,
          u * u * p0.1 + 2. * u * t * p1.1 + t * t * p2.1,
          2. * u * (p1.0 - p0.0) + 2. * t * (p2.0 - p1.0),
          2. * u * (p1.1 - p0.1) + 2. * t * (p2.1 - p1.1),
        )
      })
    }
    PathSegment::Cubic {
      start,
      ctrl1,
      ctrl2,
      end,
      ..
    } => {
      let (p0, p1, p2, p3) = (v(start), v(ctrl1), v(ctrl2), v(end));
      gauss_moments(0., 1., |t| {
        let u = 1. - t;
        let (b0, b1, b2, b3) = (u * u * u, 3. * u * u * t, 3. * u * t * t, t * t * t);
        let (d0, d1, d2) = (3. * u * u, 6. * u * t, 3. * t * t);
        (
          b0 * p0.0 + b1 * p1.0 + b2 * p2.0 + b3 * p3.0,
          b0 * p0.1 + b1 * p1.1 + b2 * p2.1 + b3 * p3.1,
          d0 * (p1.0 - p0.0) + d1 * (p2.0 - p1.0) + d2 * (p3.0 - p2.0),
          d0 * (p1.1 - p0.1) + d1 * (p2.1 - p1.1) + d2 * (p3.1 - p2.1),
        )
      })
    }
    PathSegment::Arc {
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      ..
    } => {
      let (cx, cy) = v(center);
      let (rx, ry, cp, sp) = (*rx as f64, *ry as f64, *cos_phi as f64, *sin_phi as f64);
      let (ts, td) = (*theta_start as f64, *theta_delta as f64);
      let pieces = ((td.abs() / FRAC_PI_2).ceil() as usize).max(1);
      let mut acc = (0., 0., 0.);
      for i in 0..pieces {
        let (t0, t1) = (i as f64 / pieces as f64, (i + 1) as f64 / pieces as f64);
        let m = gauss_moments(t0, t1, |t| {
          let (s, c) = (ts + td * t).sin_cos();
          let (x, y, dx, dy) = (rx * c, ry * s, -rx * s * td, ry * c * td);
          (
            cp * x - sp * y + cx,
            sp * x + cp * y + cy,
            cp * dx - sp * dy,
            sp * dx + cp * dy,
          )
        });
        acc = (acc.0 + m.0, acc.1 + m.1, acc.2 + m.2);
      }
      acc
    }
  }
}

pub(crate) fn segments_signed_area(segments: &[PathSegment]) -> f32 {
  segments.iter().map(|s| segment_moments(s).0).sum::<f64>() as f32
}

/// Area-weighted centroid of the region bounded by closed leaves; `None` when the signed area
/// nets out to ~0.
pub(crate) fn region_centroid<'a>(leaves: impl Iterator<Item = &'a [PathSegment]>) -> Option<Vec2> {
  let mut acc = (0., 0., 0.);
  for segs in leaves {
    for m in segs.iter().map(segment_moments) {
      acc = (acc.0 + m.0, acc.1 + m.1, acc.2 + m.2);
    }
  }
  (acc.0.abs() > 1e-12).then(|| {
    Vec2::new(
      (acc.1 / (2. * acc.0)) as f32,
      (-acc.2 / (2. * acc.0)) as f32,
    )
  })
}

pub(crate) fn arc_length_centroid<'a>(
  segments: impl Iterator<Item = &'a PathSegment>,
) -> Option<Vec2> {
  const N: usize = 32;
  let (mut moment, mut total) = (Vec2::zeros(), 0.);
  for seg in segments {
    let len = seg.length();
    if len <= LENGTH_EPSILON {
      continue;
    }
    let mean = match seg {
      PathSegment::Line { start, end, .. } => (start + end) * 0.5,
      _ => {
        (0..N)
          .map(|j| seg.sample_by_length(len * (j as f32 + 0.5) / N as f32))
          .sum::<Vec2>()
          / N as f32
      }
    };
    moment += mean * len;
    total += len;
  }
  (total > LENGTH_EPSILON).then(|| moment / total)
}
