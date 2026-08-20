//! The single home for texture inner loops: contiguous 1-channel slice passes that LLVM
//! auto-vectorizes. Texture builtins wrap these; the autovec executor
//! (docs/texture-autovec-plan.md) calls the same loops into its register file. Kernels must
//! call the same scalar libm as the interpreter path — bit-exact output is a contract.

use crate::{
  noise::{fbm_2d, fbm_2d_tileable},
  Vec2,
};

pub(crate) fn map_new(a: &[f32], f: impl Fn(f32) -> f32) -> Vec<f32> {
  a.iter().map(|&x| f(x)).collect()
}

pub(crate) fn map_in(a: &mut [f32], f: impl Fn(f32) -> f32) {
  for x in a {
    *x = f(*x);
  }
}

pub(crate) fn zip_new(a: &[f32], b: &[f32], f: impl Fn(f32, f32) -> f32) -> Vec<f32> {
  debug_assert_eq!(a.len(), b.len());
  a.iter().zip(b).map(|(&x, &y)| f(x, y)).collect()
}

pub(crate) fn zip_in_a(a: &mut [f32], b: &[f32], f: impl Fn(f32, f32) -> f32) {
  debug_assert_eq!(a.len(), b.len());
  for (x, &y) in a.iter_mut().zip(b) {
    *x = f(*x, y);
  }
}

pub(crate) fn zip_in_b(a: &[f32], b: &mut [f32], f: impl Fn(f32, f32) -> f32) {
  debug_assert_eq!(a.len(), b.len());
  for (&x, y) in a.iter().zip(b) {
    *y = f(x, *y);
  }
}

pub(crate) fn map_out(out: &mut [f32], a: &[f32], f: impl Fn(f32) -> f32) {
  debug_assert_eq!(out.len(), a.len());
  for (o, &x) in out.iter_mut().zip(a) {
    *o = f(x);
  }
}

pub(crate) fn zip_out(out: &mut [f32], a: &[f32], b: &[f32], f: impl Fn(f32, f32) -> f32) {
  let n = out.len();
  let (a, b) = (&a[..n], &b[..n]);
  for i in 0..n {
    out[i] = f(a[i], b[i]);
  }
}

pub(crate) fn zip3_out(
  out: &mut [f32],
  a: &[f32],
  b: &[f32],
  c: &[f32],
  f: impl Fn(f32, f32, f32) -> f32,
) {
  let n = out.len();
  let (a, b, c) = (&a[..n], &b[..n], &c[..n]);
  for i in 0..n {
    out[i] = f(a[i], b[i], c[i]);
  }
}

pub(crate) fn zip3_new(
  a: &[f32],
  b: &[f32],
  c: &[f32],
  f: impl Fn(f32, f32, f32) -> f32,
) -> Vec<f32> {
  let n = a.len().min(b.len()).min(c.len());
  let (a, b, c) = (&a[..n], &b[..n], &c[..n]);
  (0..n).map(|i| f(a[i], b[i], c[i])).collect()
}

pub(crate) fn mul_acc(acc: &mut [f32], a: &[f32], b: &[f32]) {
  let n = acc.len();
  let (a, b) = (&a[..n], &b[..n]);
  for i in 0..n {
    acc[i] += a[i] * b[i];
  }
}

pub(crate) fn diff_sq_acc(acc: &mut [f32], a: &[f32], b: &[f32]) {
  let n = acc.len();
  let (a, b) = (&a[..n], &b[..n]);
  for i in 0..n {
    let d = a[i] - b[i];
    acc[i] += d * d;
  }
}

/// Exact per-lane pick for the vectorizer's `select` (plan doc, Conditionals §2): bitwise,
/// never arithmetic. The branchy form doesn't if-convert in wasm (scalar loop, ~5×) and a
/// lerp form would corrupt NaN payloads and signed zeros.
#[inline(always)]
pub(crate) fn bitsel(m: f32, a: f32, b: f32) -> f32 {
  let mask = ((m != 0.) as u32).wrapping_neg();
  f32::from_bits((a.to_bits() & mask) | (b.to_bits() & !mask))
}

/// Dense fbm over a varying vec2 position (planar x/y), uniform kwargs — the whitelisted
/// noise kernel for texel bodies like `fbm(pos=uv + ...)`.
#[allow(dead_code)]
pub(crate) fn fbm2_kern(
  out: &mut [f32],
  px: &[f32],
  py: &[f32],
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  period: Option<f32>,
) {
  let n = out.len();
  let (px, py) = (&px[..n], &py[..n]);
  match period {
    Some(p) => {
      for i in 0..n {
        out[i] = fbm_2d_tileable(
          seed,
          octaves,
          frequency,
          persistence,
          lacunarity,
          p,
          Vec2::new(px[i], py[i]),
        );
      }
    }
    None => {
      for i in 0..n {
        out[i] = fbm_2d(
          seed,
          octaves,
          frequency,
          persistence,
          lacunarity,
          Vec2::new(px[i], py[i]),
        );
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn select_is_bit_exact() {
    let m = [1., 0., 2., 0.];
    let a = [f32::from_bits(0x7fc00001), -0.0, 1.5, f32::INFINITY];
    let b = [9., f32::from_bits(0x7fc00002), 8., -0.0];
    let out: Vec<f32> = (0..4).map(|i| bitsel(m[i], a[i], b[i])).collect();
    assert_eq!(out[0].to_bits(), 0x7fc00001);
    assert_eq!(out[1].to_bits(), 0x7fc00002);
    assert_eq!(out[2], 1.5);
    assert_eq!(out[3].to_bits(), (-0.0f32).to_bits());
  }

  #[test]
  fn fbm2_kern_matches_scalar() {
    let px: Vec<f32> = (0..16).map(|i| i as f32 * 0.37).collect();
    let py: Vec<f32> = (0..16).map(|i| i as f32 * -0.13 + 0.5).collect();
    let mut out = vec![0f32; 16];
    fbm2_kern(&mut out, &px, &py, 7, 5, 3., 0.5, 2., None);
    for i in 0..16 {
      assert_eq!(
        out[i].to_bits(),
        fbm_2d(7, 5, 3., 0.5, 2., Vec2::new(px[i], py[i])).to_bits()
      );
    }
    fbm2_kern(&mut out, &px, &py, 7, 5, 3., 0.5, 2., Some(4.));
    for i in 0..16 {
      assert_eq!(
        out[i].to_bits(),
        fbm_2d_tileable(7, 5, 3., 0.5, 2., 4., Vec2::new(px[i], py[i])).to_bits()
      );
    }
  }
}
