use std::f32::consts::PI;
use std::ops::{Add, Mul, Sub};

use mesh::linked_mesh::Vec3;

use crate::Vec2;

pub fn build_torus_knot_path(
  radius: f32,
  tube_radius: f32,
  p: usize,
  q: usize,
  count: usize,
) -> impl Iterator<Item = Vec3> + Clone {
  pub fn sample_torus_knot(p: usize, q: usize, radius: f32, tube_radius: f32, t: f32) -> Vec3 {
    let t = 2. * PI * t;
    let p = p as f32;
    let q = q as f32;
    let qt = q * t;
    let pt = p * t;
    let radius = radius + tube_radius * qt.cos();
    let x = radius * pt.cos();
    let y = radius * pt.sin();
    let z = tube_radius * qt.sin();
    Vec3::new(x, y, z)
  }

  (0..=count).map(move |i| {
    let t = i as f32 / count as f32;
    sample_torus_knot(p, q, radius, tube_radius, t)
  })
}

pub fn build_lissajous_knot_path(
  amp: Vec3,
  freq: Vec3,
  phase: Vec3,
  count: usize,
) -> impl Iterator<Item = Vec3> + Clone {
  #[inline(always)]
  fn sample_lissajous(amp: Vec3, freq: Vec3, phase: Vec3, t: f32) -> Vec3 {
    let t = 2. * PI * t;

    Vec3::new(
      amp.x * (freq[0] * t + phase.x).sin(),
      amp.y * (freq[1] * t + phase.y).sin(),
      amp.z * (freq[2] * t + phase.z).sin(),
    )
  }

  (0..count).map(move |i| {
    let t = i as f32 / count as f32;
    sample_lissajous(amp, freq, phase, t)
  })
}

fn cubic_bezier_3d(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
  let u = 1. - t;
  let tt = t * t;
  let uu = u * u;
  let uuu = uu * u;
  let ttt = tt * t;

  uuu * p0 + 3. * uu * t * p1 + 3. * u * tt * p2 + ttt * p3
}

pub fn cubic_bezier_3d_path(
  p0: Vec3,
  p1: Vec3,
  p2: Vec3,
  p3: Vec3,
  count: usize,
) -> impl Iterator<Item = Vec3> + Clone + 'static {
  (0..=count).map(move |i| {
    let t = i as f32 / count as f32;
    cubic_bezier_3d(p0, p1, p2, p3, t)
  })
}

/// Evaluates a single cubic Hermite segment.
/// `p0`/`p1` are the start/end points; `t0`/`t1` are the tangents; `s ∈ [0, 1]`.
#[inline]
fn eval_hermite_segment<P>(p0: P, p1: P, t0: P, t1: P, s: f32) -> P
where
  P: Copy + Add<Output = P> + Mul<f32, Output = P>,
{
  let s2 = s * s;
  let s3 = s2 * s;
  p0 * (2. * s3 - 3. * s2 + 1.)
    + p1 * (-2. * s3 + 3. * s2)
    + t0 * (s3 - 2. * s2 + s)
    + t1 * (s3 - s2)
}

/// Evaluates a cardinal spline at `t ∈ [0, 1]`.
///
/// A cardinal spline is a cubic Hermite spline whose tangents are derived automatically from
/// neighbouring control points:  `T_i = tension * (P_{i+1} - P_{i-1})`.  Setting
/// `tension = 0.5` gives a standard Catmull-Rom spline.
///
/// For **open** splines the tangents at the two endpoints are computed using a clamped
/// phantom neighbour (`P_{-1} = P_0`, `P_n = P_{n-1}`), which makes the endpoint tangents
/// point toward the adjacent interior point scaled by `tension`.
///
/// For **closed** splines the index arithmetic wraps around so the curve joins smoothly
/// at `t = 0`/`t = 1`.
///
/// Requires at least 2 control points.
pub fn eval_cardinal_spline<P>(points: &[P], t: f32, tension: f32, closed: bool) -> P
where
  P: Copy + Add<Output = P> + Sub<Output = P> + Mul<f32, Output = P>,
{
  let n = points.len();
  debug_assert!(n >= 2, "cardinal spline requires at least 2 control points");

  let n_segs = if closed { n } else { n - 1 };
  let raw = (t * n_segs as f32).clamp(0., n_segs as f32);
  let seg = (raw.floor() as usize).min(n_segs - 1);
  let s = raw - seg as f32;

  let get = |i: isize| -> P {
    if closed {
      points[i.rem_euclid(n as isize) as usize]
    } else {
      points[i.clamp(0, (n as isize) - 1) as usize]
    }
  };

  let i = seg as isize;
  let p_prev = get(i - 1);
  let p0 = get(i);
  let p1 = get(i + 1);
  let p_next = get(i + 2);

  let tan0 = (p1 - p_prev) * tension;
  let tan1 = (p_next - p0) * tension;

  eval_hermite_segment(p0, p1, tan0, tan1, s)
}

pub fn get_superellipse_point(t: f32, width: f32, height: f32, n: f32) -> (f32, f32) {
  let (sin_theta, cos_theta) = (t * 2. * PI).sin_cos();
  let pow = 2. / n;
  let x = (width / 2.) * cos_theta.signum() * cos_theta.abs().powf(pow);
  let y = (height / 2.) * sin_theta.signum() * sin_theta.abs().powf(pow);
  (x, y)
}

pub fn superellipse_path(
  width: f32,
  height: f32,
  n: f32,
  count: usize,
) -> impl Iterator<Item = Vec2> + Clone + 'static {
  (0..=count).map(move |i| {
    let t = i as f32 / count as f32;
    let (x, y) = get_superellipse_point(t, width, height, n);
    Vec2::new(x, y)
  })
}
