use std::cmp::Ordering;
use std::f32::consts::PI;

use nalgebra::{Matrix3, Vector3};

use crate::{ErrorStack, Vec2};

pub(crate) const CURVE_TABLE_SAMPLES: usize = 32;
pub(crate) const LENGTH_EPSILON: f32 = 1e-5;

#[derive(Clone, Debug)]
pub(crate) struct ArcLengthTable {
  cumulative: Vec<f32>,
  total: f32,
}

impl ArcLengthTable {
  pub(crate) fn new(samples: usize, mut sample_fn: impl FnMut(f32) -> Vec2) -> Self {
    let samples = samples.max(1);
    let mut cumulative = Vec::with_capacity(samples + 1);
    let mut total = 0.0;

    let mut prev = sample_fn(0.0);
    cumulative.push(0.0);

    for i in 1..=samples {
      let t = i as f32 / samples as f32;
      let point = sample_fn(t);
      total += (point - prev).norm();
      cumulative.push(total);
      prev = point;
    }

    Self { cumulative, total }
  }

  pub(crate) fn total(&self) -> f32 {
    self.total
  }

  pub(crate) fn param_for_length(&self, length: f32) -> f32 {
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

#[derive(Clone, Debug)]
pub(crate) enum PathSegment {
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

/// Applies a 2D affine transform (3x3 homogeneous matrix) to a point.
/// Returns the point unchanged if the matrix is identity.
pub(crate) fn apply_transform_to_point(m: &Matrix3<f32>, p: Vec2) -> Vec2 {
  if *m == Matrix3::identity() {
    return p;
  }
  let tp = m * Vector3::new(p.x, p.y, 1.0);
  Vec2::new(tp.x, tp.y)
}

impl PathSegment {
  pub(crate) fn translate(&mut self, offset: Vec2) {
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

  pub(crate) fn length(&self) -> f32 {
    match self {
      PathSegment::Line { length, .. } => *length,
      PathSegment::Quadratic { table, .. } => table.total(),
      PathSegment::Cubic { table, .. } => table.total(),
      PathSegment::Arc { table, .. } => table.total(),
    }
  }

  pub(crate) fn has_detail(&self) -> bool {
    !matches!(self, PathSegment::Line { .. })
  }

  pub(crate) fn end(&self) -> Vec2 {
    match self {
      PathSegment::Line { end, .. } => *end,
      PathSegment::Quadratic { end, .. } => *end,
      PathSegment::Cubic { end, .. } => *end,
      PathSegment::Arc { end, .. } => *end,
    }
  }

  pub(crate) fn start_point(&self) -> Vec2 {
    match self {
      PathSegment::Line { start, .. } => *start,
      PathSegment::Quadratic { start, .. } => *start,
      PathSegment::Cubic { start, .. } => *start,
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        ..
      } => {
        let (sin_theta, cos_theta) = theta_start.sin_cos();
        let x = rx * cos_theta;
        let y = ry * sin_theta;
        Vec2::new(
          cos_phi * x - sin_phi * y + center.x,
          sin_phi * x + cos_phi * y + center.y,
        )
      }
    }
  }

  /// Returns the exact AABB of this segment in its local coordinate space.
  pub(crate) fn aabb(&self) -> (Vec2, Vec2) {
    match self {
      PathSegment::Line { start, end, .. } => (
        Vec2::new(start.x.min(end.x), start.y.min(end.y)),
        Vec2::new(start.x.max(end.x), start.y.max(end.y)),
      ),
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => quadratic_bezier_aabb(*start, *ctrl, *end),
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => cubic_bezier_aabb(*start, *ctrl1, *ctrl2, *end),
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        theta_delta,
        ..
      } => arc_aabb(
        *center,
        *rx,
        *ry,
        *cos_phi,
        *sin_phi,
        *theta_start,
        *theta_delta,
      ),
    }
  }

  /// Returns the exact AABB of this segment after applying the given 2D affine transform.
  /// Errors only for arc segments under a non-uniform transform, where the result is not an
  /// arc and an exact bound would require evaluating a transformed conic.
  pub(crate) fn aabb_under_transform(&self, m: &Matrix3<f32>) -> Result<(Vec2, Vec2), ErrorStack> {
    if *m == Matrix3::identity() {
      return Ok(self.aabb());
    }
    match self {
      PathSegment::Line { start, end, .. } => {
        let s = apply_transform_to_point(m, *start);
        let e = apply_transform_to_point(m, *end);
        Ok((
          Vec2::new(s.x.min(e.x), s.y.min(e.y)),
          Vec2::new(s.x.max(e.x), s.y.max(e.y)),
        ))
      }
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => Ok(quadratic_bezier_aabb(
        apply_transform_to_point(m, *start),
        apply_transform_to_point(m, *ctrl),
        apply_transform_to_point(m, *end),
      )),
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => Ok(cubic_bezier_aabb(
        apply_transform_to_point(m, *start),
        apply_transform_to_point(m, *ctrl1),
        apply_transform_to_point(m, *ctrl2),
        apply_transform_to_point(m, *end),
      )),
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
        if !is_uniform_transform(m) {
          return Err(ErrorStack::new(
            "exact AABB of an arc segment under a non-uniform transform (e.g. non-uniform scale \
             or skew) is not supported; bake the transform into the path first or convert arcs to \
             cubic beziers",
          ));
        }
        let cos_a = m[(0, 0)];
        let sin_a = m[(1, 0)];
        let scale = (cos_a * cos_a + sin_a * sin_a).sqrt();
        let rot_angle = sin_a.atan2(cos_a);
        let new_center = apply_transform_to_point(m, *center);
        let new_rx = rx * scale;
        let new_ry = ry * scale;
        let old_phi = sin_phi.atan2(*cos_phi);
        let new_phi = old_phi + rot_angle;
        Ok(arc_aabb(
          new_center,
          new_rx,
          new_ry,
          new_phi.cos(),
          new_phi.sin(),
          *theta_start,
          *theta_delta,
        ))
      }
    }
  }

  pub(crate) fn sample_by_length(&self, length: f32) -> Vec2 {
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

  /// Maps a local arc length into the segment's native curve parameter in [0, 1].
  pub(crate) fn param_for_length(&self, length: f32) -> f32 {
    match self {
      PathSegment::Line {
        length: seg_len, ..
      } => {
        if *seg_len <= LENGTH_EPSILON {
          0.0
        } else {
          (length / *seg_len).clamp(0.0, 1.0)
        }
      }
      PathSegment::Quadratic { table, .. }
      | PathSegment::Cubic { table, .. }
      | PathSegment::Arc { table, .. } => table.param_for_length(length),
    }
  }

  /// Returns the sub-segment covering the native-parameter range `[u, v]` (with `0 <= u <= v <=
  /// 1`), preserving the curve type exactly (lines stay lines, beziers stay beziers, arcs stay
  /// arcs). `None` when the resulting piece is degenerate.
  pub(crate) fn slice(&self, u: f32, v: f32) -> Option<PathSegment> {
    if u <= LENGTH_EPSILON && v >= 1.0 - LENGTH_EPSILON {
      return Some(self.clone());
    }
    match self {
      PathSegment::Line { start, end, .. } => {
        let s = *start + (*end - *start) * u;
        let e = *start + (*end - *start) * v;
        let length = (e - s).norm();
        (length > LENGTH_EPSILON).then_some(PathSegment::Line {
          start: s,
          end: e,
          length,
        })
      }
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => {
        let (s, c, e) = quadratic_subsegment(*start, *ctrl, *end, u, v);
        let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| quadratic_bezier(s, c, e, t));
        (table.total() > LENGTH_EPSILON).then_some(PathSegment::Quadratic {
          start: s,
          ctrl: c,
          end: e,
          table,
        })
      }
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => {
        let (s, c1, c2, e) = cubic_subsegment(*start, *ctrl1, *ctrl2, *end, u, v);
        let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| cubic_bezier(s, c1, c2, e, t));
        (table.total() > LENGTH_EPSILON).then_some(PathSegment::Cubic {
          start: s,
          ctrl1: c1,
          ctrl2: c2,
          end: e,
          table,
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
        let new_theta_start = theta_start + theta_delta * u;
        let new_theta_delta = theta_delta * (v - u);
        let new_end = arc_point(
          *center,
          *rx,
          *ry,
          *cos_phi,
          *sin_phi,
          *theta_start,
          *theta_delta,
          v,
        );
        let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
          arc_point(
            *center,
            *rx,
            *ry,
            *cos_phi,
            *sin_phi,
            new_theta_start,
            new_theta_delta,
            t,
          )
        });
        (table.total() > LENGTH_EPSILON).then_some(PathSegment::Arc {
          end: new_end,
          center: *center,
          rx: *rx,
          ry: *ry,
          cos_phi: *cos_phi,
          sin_phi: *sin_phi,
          theta_start: new_theta_start,
          theta_delta: new_theta_delta,
          table,
        })
      }
    }
  }
}

/// Returns the control points of the two halves of a quadratic bezier split at parameter `t`.
pub(crate) fn split_quadratic(
  p0: Vec2,
  p1: Vec2,
  p2: Vec2,
  t: f32,
) -> ((Vec2, Vec2, Vec2), (Vec2, Vec2, Vec2)) {
  let a = p0 + (p1 - p0) * t;
  let b = p1 + (p2 - p1) * t;
  let m = a + (b - a) * t;
  ((p0, a, m), (m, b, p2))
}

/// Control points of the quadratic sub-bezier on `[u, v]` (`0 <= u <= v <= 1`).
pub(crate) fn quadratic_subsegment(
  p0: Vec2,
  p1: Vec2,
  p2: Vec2,
  u: f32,
  v: f32,
) -> (Vec2, Vec2, Vec2) {
  let ((l0, l1, l2), _) = split_quadratic(p0, p1, p2, v);
  let uu = if v > LENGTH_EPSILON {
    (u / v).clamp(0.0, 1.0)
  } else {
    0.0
  };
  let (_, right) = split_quadratic(l0, l1, l2, uu);
  right
}

pub(crate) fn split_cubic(
  p0: Vec2,
  p1: Vec2,
  p2: Vec2,
  p3: Vec2,
  t: f32,
) -> ((Vec2, Vec2, Vec2, Vec2), (Vec2, Vec2, Vec2, Vec2)) {
  let a = p0 + (p1 - p0) * t;
  let b = p1 + (p2 - p1) * t;
  let c = p2 + (p3 - p2) * t;
  let d = a + (b - a) * t;
  let e = b + (c - b) * t;
  let m = d + (e - d) * t;
  ((p0, a, d, m), (m, e, c, p3))
}

/// Control points of the cubic sub-bezier on `[u, v]` (`0 <= u <= v <= 1`).
pub(crate) fn cubic_subsegment(
  p0: Vec2,
  p1: Vec2,
  p2: Vec2,
  p3: Vec2,
  u: f32,
  v: f32,
) -> (Vec2, Vec2, Vec2, Vec2) {
  let ((l0, l1, l2, l3), _) = split_cubic(p0, p1, p2, p3, v);
  let uu = if v > LENGTH_EPSILON {
    (u / v).clamp(0.0, 1.0)
  } else {
    0.0
  };
  let (_, right) = split_cubic(l0, l1, l2, l3, uu);
  right
}

/// Returns the analytic 2D AABB of a quadratic bezier with the given control points.
///
/// Solves dB/dt = 0 per component for the interior extrema, evaluates the curve at those
/// in-range t values, and combines with the endpoints. Exact modulo floating-point rounding.
pub(crate) fn quadratic_bezier_aabb(p0: Vec2, p1: Vec2, p2: Vec2) -> (Vec2, Vec2) {
  let mut min = Vec2::new(p0.x.min(p2.x), p0.y.min(p2.y));
  let mut max = Vec2::new(p0.x.max(p2.x), p0.y.max(p2.y));
  for axis in 0..2 {
    let a0 = p0[axis];
    let a1 = p1[axis];
    let a2 = p2[axis];
    let denom = a0 - 2.0 * a1 + a2;
    if denom.abs() <= 1e-12 {
      continue;
    }
    let t = (a0 - a1) / denom;
    if (0.0..=1.0).contains(&t) {
      let v = quadratic_bezier(p0, p1, p2, t)[axis];
      min[axis] = min[axis].min(v);
      max[axis] = max[axis].max(v);
    }
  }
  (min, max)
}

/// Returns the analytic 2D AABB of a cubic bezier with the given control points.
///
/// Solves the quadratic dB/dt = 0 per component for interior extrema (up to two per axis),
/// evaluates the curve at those in-range t values, and combines with the endpoints. Exact
/// modulo floating-point rounding.
pub(crate) fn cubic_bezier_aabb(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> (Vec2, Vec2) {
  let mut min = Vec2::new(p0.x.min(p3.x), p0.y.min(p3.y));
  let mut max = Vec2::new(p0.x.max(p3.x), p0.y.max(p3.y));
  for axis in 0..2 {
    let a0 = p0[axis];
    let a1 = p1[axis];
    let a2 = p2[axis];
    let a3 = p3[axis];
    // B'(t) / 3 = (1-t)^2 (a1-a0) + 2(1-t)t (a2-a1) + t^2 (a3-a2)
    //           = a t^2 + b t + c
    let a = -a0 + 3.0 * a1 - 3.0 * a2 + a3;
    let b = 2.0 * (a0 - 2.0 * a1 + a2);
    let c = a1 - a0;

    let mut consider = |t: f32| {
      if (0.0..=1.0).contains(&t) {
        let v = cubic_bezier(p0, p1, p2, p3, t)[axis];
        min[axis] = min[axis].min(v);
        max[axis] = max[axis].max(v);
      }
    };

    if a.abs() <= 1e-12 {
      if b.abs() > 1e-12 {
        consider(-c / b);
      }
      continue;
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
      continue;
    }
    let sq = disc.sqrt();
    consider((-b + sq) / (2.0 * a));
    consider((-b - sq) / (2.0 * a));
  }
  (min, max)
}

/// Returns the analytic 2D AABB of the elliptical arc covered by the given parametrization.
///
/// `theta_delta` may be positive or negative (signed sweep). Critical angles where
/// dx/dθ = 0 or dy/dθ = 0 are computed analytically; each is shifted by multiples of 2π
/// to test inclusion in the swept range, and contributing points are folded into the
/// endpoint bounds. Exact modulo floating-point rounding.
pub(crate) fn arc_aabb(
  center: Vec2,
  rx: f32,
  ry: f32,
  cos_phi: f32,
  sin_phi: f32,
  theta_start: f32,
  theta_delta: f32,
) -> (Vec2, Vec2) {
  let start = arc_point(
    center,
    rx,
    ry,
    cos_phi,
    sin_phi,
    theta_start,
    theta_delta,
    0.0,
  );
  let end = arc_point(
    center,
    rx,
    ry,
    cos_phi,
    sin_phi,
    theta_start,
    theta_delta,
    1.0,
  );
  let mut min = Vec2::new(start.x.min(end.x), start.y.min(end.y));
  let mut max = Vec2::new(start.x.max(end.x), start.y.max(end.y));

  if theta_delta.abs() <= 1e-12 || rx <= 0.0 || ry <= 0.0 {
    return (min, max);
  }

  let theta_x = (-ry * sin_phi).atan2(rx * cos_phi);
  let theta_y = (ry * cos_phi).atan2(rx * sin_phi);

  let two_pi = std::f32::consts::TAU;
  let theta_end = theta_start + theta_delta;
  let (theta_lo, theta_hi) = if theta_delta >= 0.0 {
    (theta_start, theta_end)
  } else {
    (theta_end, theta_start)
  };

  for theta_c in [
    theta_x,
    theta_x + std::f32::consts::PI,
    theta_y,
    theta_y + std::f32::consts::PI,
  ] {
    // Shift theta_c by 2π·k so it falls into [theta_lo, theta_hi], if possible.
    let k = ((theta_lo - theta_c) / two_pi).ceil();
    let theta_in = theta_c + k * two_pi;
    if theta_in < theta_lo - 1e-6 || theta_in > theta_hi + 1e-6 {
      continue;
    }
    let t = (theta_in - theta_start) / theta_delta;
    if !(-1e-6..=1.0 + 1e-6).contains(&t) {
      continue;
    }
    let p = arc_point(
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      t.clamp(0.0, 1.0),
    );
    min.x = min.x.min(p.x);
    min.y = min.y.min(p.y);
    max.x = max.x.max(p.x);
    max.y = max.y.max(p.y);
  }

  (min, max)
}

pub(crate) fn is_uniform_transform(m: &Matrix3<f32>) -> bool {
  let a = m[(0, 0)];
  let b = m[(0, 1)];
  let c = m[(1, 0)];
  let d = m[(1, 1)];
  let eps = 1e-5;
  // For a scaled rotation: a == d && b == -c
  (a - d).abs() < eps && (b + c).abs() < eps
}

pub(crate) fn transform_segment(
  seg: &PathSegment,
  m: &Matrix3<f32>,
) -> Result<PathSegment, ErrorStack> {
  match seg {
    PathSegment::Line { start, end, .. } => {
      let new_start = apply_transform_to_point(m, *start);
      let new_end = apply_transform_to_point(m, *end);
      let length = (new_end - new_start).norm();
      Ok(PathSegment::Line {
        start: new_start,
        end: new_end,
        length,
      })
    }
    PathSegment::Quadratic {
      start, ctrl, end, ..
    } => {
      let new_start = apply_transform_to_point(m, *start);
      let new_ctrl = apply_transform_to_point(m, *ctrl);
      let new_end = apply_transform_to_point(m, *end);
      let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
        quadratic_bezier(new_start, new_ctrl, new_end, t)
      });
      Ok(PathSegment::Quadratic {
        start: new_start,
        ctrl: new_ctrl,
        end: new_end,
        table,
      })
    }
    PathSegment::Cubic {
      start,
      ctrl1,
      ctrl2,
      end,
      ..
    } => {
      let new_start = apply_transform_to_point(m, *start);
      let new_ctrl1 = apply_transform_to_point(m, *ctrl1);
      let new_ctrl2 = apply_transform_to_point(m, *ctrl2);
      let new_end = apply_transform_to_point(m, *end);
      let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
        cubic_bezier(new_start, new_ctrl1, new_ctrl2, new_end, t)
      });
      Ok(PathSegment::Cubic {
        start: new_start,
        ctrl1: new_ctrl1,
        ctrl2: new_ctrl2,
        end: new_end,
        table,
      })
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
      if !is_uniform_transform(m) {
        return Err(ErrorStack::new(
          "apply_transforms: cannot bake a non-uniform transform (e.g. non-uniform scale or skew) \
           into a path that contains arc segments. Arc segments can only be exactly preserved \
           under uniform transforms (translation, rotation, uniform scale). Consider using \
           `path_scale` with a uniform factor, or convert arcs to cubic beziers first.",
        ));
      }

      // Uniform similarity transform: preserves arcs exactly.
      // Extract the uniform scale and rotation angle from the 2x2 block. For the project's
      // row-major rotation matrix `[cos, -sin; sin, cos]`, the signed rotation angle comes
      // from `atan2(m[1,0], m[0,0]) = atan2(sin, cos)`; using `m[0,1]` would yield -angle.
      let cos_a = m[(0, 0)];
      let sin_a = m[(1, 0)];
      let scale = (cos_a * cos_a + sin_a * sin_a).sqrt();
      let rot_angle = sin_a.atan2(cos_a);

      let new_center = apply_transform_to_point(m, *center);
      let new_end = apply_transform_to_point(m, *end);
      let new_rx = rx * scale;
      let new_ry = ry * scale;

      // Compose the rotation into the existing ellipse rotation (phi)
      let old_phi = sin_phi.atan2(*cos_phi);
      let new_phi = old_phi + rot_angle;
      let new_cos_phi = new_phi.cos();
      let new_sin_phi = new_phi.sin();

      let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
        arc_point(
          new_center,
          new_rx,
          new_ry,
          new_cos_phi,
          new_sin_phi,
          *theta_start,
          *theta_delta,
          t,
        )
      });

      Ok(PathSegment::Arc {
        end: new_end,
        center: new_center,
        rx: new_rx,
        ry: new_ry,
        cos_phi: new_cos_phi,
        sin_phi: new_sin_phi,
        theta_start: *theta_start,
        theta_delta: *theta_delta,
        table,
      })
    }
  }
}

pub(crate) fn angle_between(a: Vec2, b: Vec2) -> f32 {
  let la = a.norm();
  let lb = b.norm();
  if la <= LENGTH_EPSILON || lb <= LENGTH_EPSILON {
    return 0.0;
  }
  let cos = (a.dot(&b) / (la * lb)).clamp(-1.0, 1.0);
  cos.acos()
}

pub(crate) fn segment_turning_angle(seg: &PathSegment) -> f32 {
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

/// Chords turn at most `angle_tolerance` and, with arc-length-uniform subdivision, sag at most
/// `max_sagitta` (~ L*phi/(8n^2)); `INFINITY` disables the sagitta term.
pub(crate) fn segment_subdivisions(
  seg: &PathSegment,
  angle_tolerance: f32,
  max_sagitta: f32,
) -> usize {
  if angle_tolerance <= 0.0 || !seg.has_detail() {
    return 1;
  }
  let angle = segment_turning_angle(seg);
  if angle <= 0.0 {
    return 1;
  }
  let by_angle = (angle / angle_tolerance).ceil() as usize;
  let by_sagitta = (seg.length() * angle / (8. * max_sagitta)).sqrt().ceil() as usize;
  by_angle.max(by_sagitta).max(1)
}

pub(crate) fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, t: f32) -> Vec2 {
  let u = 1.0 - t;
  let tt = t * t;
  let uu = u * u;
  uu * p0 + 2.0 * u * t * p1 + tt * p2
}

pub(crate) fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
  let u = 1.0 - t;
  let tt = t * t;
  let uu = u * u;
  let uuu = uu * u;
  let ttt = tt * t;
  uuu * p0 + 3.0 * uu * t * p1 + 3.0 * u * tt * p2 + ttt * p3
}

pub(crate) fn arc_point(
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

pub(crate) fn build_arc_segment(
  start: Vec2,
  end: Vec2,
  rx: f32,
  ry: f32,
  x_axis_rotation: f32,
  large_arc: bool,
  sweep: bool,
) -> Option<PathSegment> {
  let mut rx = rx.abs();
  let mut ry = ry.abs();
  if rx <= LENGTH_EPSILON || ry <= LENGTH_EPSILON {
    let length = (end - start).norm();
    return Some(PathSegment::Line { start, end, length });
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
  // Fourth-power quantity: an absolute epsilon here silently flattened every small arc.
  let denom = rx_sq * y1p_sq + ry_sq * x1p_sq;
  if denom <= 0.0 {
    let length = (end - start).norm();
    return Some(PathSegment::Line { start, end, length });
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

  let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
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

  Some(PathSegment::Arc {
    end,
    center,
    rx,
    ry,
    cos_phi,
    sin_phi,
    theta_start,
    theta_delta,
    table,
  })
}

pub(crate) fn quad_segment(start: Vec2, ctrl: Vec2, end: Vec2) -> PathSegment {
  PathSegment::Quadratic {
    start,
    ctrl,
    end,
    table: ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
      quadratic_bezier(start, ctrl, end, t)
    }),
  }
}

pub(crate) fn cubic_segment(start: Vec2, ctrl1: Vec2, ctrl2: Vec2, end: Vec2) -> PathSegment {
  PathSegment::Cubic {
    start,
    ctrl1,
    ctrl2,
    end,
    table: ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
      cubic_bezier(start, ctrl1, ctrl2, end, t)
    }),
  }
}

pub(crate) fn line_segment(start: Vec2, end: Vec2) -> PathSegment {
  PathSegment::Line {
    start,
    end,
    length: (end - start).norm(),
  }
}

impl PathSegment {
  pub(crate) fn reversed(&self) -> PathSegment {
    match self {
      PathSegment::Line { start, end, length } => PathSegment::Line {
        start: *end,
        end: *start,
        length: *length,
      },
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => quad_segment(*end, *ctrl, *start),
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => cubic_segment(*end, *ctrl2, *ctrl1, *start),
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
        let (c, rx, ry, cp, sp) = (*center, *rx, *ry, *cos_phi, *sin_phi);
        let (ts, td) = (theta_start + theta_delta, -theta_delta);
        PathSegment::Arc {
          end: self.start_point(),
          center: c,
          rx,
          ry,
          cos_phi: cp,
          sin_phi: sp,
          theta_start: ts,
          theta_delta: td,
          table: ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            arc_point(c, rx, ry, cp, sp, ts, td, t)
          }),
        }
      }
    }
  }

  /// Appends the transformed segment; an arc under a non-uniform transform becomes cubics.
  pub(crate) fn transform_into(&self, m: &Matrix3<f32>, out: &mut Vec<PathSegment>) {
    if let (
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        theta_delta,
        ..
      },
      false,
    ) = (self, is_uniform_transform(m))
    {
      use lyon_tessellation::geom::{Angle, Arc, Point, Vector};
      let arc = Arc {
        center: Point::new(center.x, center.y),
        radii: Vector::new(*rx, *ry),
        start_angle: Angle::radians(*theta_start),
        sweep_angle: Angle::radians(*theta_delta),
        x_rotation: Angle::radians(sin_phi.atan2(*cos_phi)),
      };
      let tx = |p: Point<f32>| apply_transform_to_point(m, Vec2::new(p.x, p.y));
      arc.for_each_cubic_bezier(&mut |c| {
        out.push(cubic_segment(
          tx(c.from),
          tx(c.ctrl1),
          tx(c.ctrl2),
          tx(c.to),
        ));
      });
      return;
    }
    out.push(transform_segment(self, m).unwrap());
  }
}
