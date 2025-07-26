use std::f32::consts::PI;

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
