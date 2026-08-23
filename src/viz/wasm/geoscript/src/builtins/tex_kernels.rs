//! The single home for texture inner loops: contiguous 1-channel slice passes that LLVM
//! auto-vectorizes. Texture builtins wrap these; the autovec executor
//! (docs/texture-autovec-plan.md) calls the same loops into its register file. Kernels must
//! call the same scalar libm as the interpreter path — bit-exact output is a contract.

use crate::{
  noise::{fbm_2d, fbm_2d_tileable},
  ErrorStack, TextureWrap, Vec2,
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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum SampleFilter {
  Nearest,
  Bilinear,
}

impl SampleFilter {
  pub(crate) fn from_name(s: &str) -> Result<Self, ErrorStack> {
    match s {
      "nearest" => Ok(Self::Nearest),
      "bilinear" => Ok(Self::Bilinear),
      _ => Err(ErrorStack::new(format!(
        "Invalid sample filter: \"{s}\"; expected \"nearest\" or \"bilinear\""
      ))),
    }
  }
}

/// Source for the gather kernels: a `w`×`h` texture's planes under strided addressing
/// (`origin + y * y_pitch + x * x_pitch`), so views gather without materializing.
#[derive(Clone, Copy)]
pub(crate) struct GatherSrc<'a> {
  pub planes: &'a [&'a [f32]],
  pub w: usize,
  pub h: usize,
  pub origin: isize,
  pub x_pitch: isize,
  pub y_pitch: isize,
  pub wrap: TextureWrap,
}

impl GatherSrc<'_> {
  #[inline(always)]
  fn at(&self, x: usize, y: usize) -> usize {
    (self.origin + y as isize * self.y_pitch + x as isize * self.x_pitch) as usize
  }
}

#[inline(always)]
fn repeat01(u: f32) -> f32 {
  u - u.floor()
}

#[inline(always)]
fn clamp01(u: f32) -> f32 {
  u.clamp(0., 1.)
}

#[inline(always)]
fn mirror01(u: f32) -> f32 {
  let t = repeat01(u * 0.5) * 2.;
  if t > 1. {
    2. - t
  } else {
    t
  }
}

/// Monomorphizes `$body` per wrap mode: `$f` canonicalizes a coordinate into [0, 1] and
/// `$rep` says whether the two bilinear taps wrap toroidally or clamp.
macro_rules! with_wrap {
  ($wrap:expr, |$f:ident, $rep:ident| $body:expr) => {
    match $wrap {
      TextureWrap::Repeat => {
        let ($f, $rep) = (repeat01, true);
        $body
      }
      TextureWrap::Clamp => {
        let ($f, $rep) = (clamp01, false);
        $body
      }
      TextureWrap::Mirror => {
        let ($f, $rep) = (mirror01, false);
        $body
      }
    }
  };
}

/// Coordinates canonicalize to [0, 1] (so `u = 1.` lands on texel 0 under repeat) and the
/// index clamps, which also absorbs `u01 * n` rounding up to exactly `n`; NaN → texel 0.
#[inline(always)]
fn nearest_ix(u: f32, n: usize, wrap01: impl Fn(f32) -> f32) -> usize {
  ((wrap01(u) * n as f32).floor() as i32).min(n as i32 - 1) as usize
}

/// Tap pair + blend weight along one axis; texel centers sit at `(i + 0.5) / n`.
#[inline(always)]
fn linear_taps(u: f32, n: usize, rep: bool, wrap01: impl Fn(f32) -> f32) -> (usize, usize, f32) {
  let s = wrap01(u) * n as f32 - 0.5;
  let s0 = s.floor();
  let i0 = s0 as i32;
  let last = n as i32 - 1;
  let (a, b) = if rep {
    (
      if i0 < 0 { last } else { i0.min(last) },
      if i0 >= last { 0 } else { i0 + 1 },
    )
  } else {
    (i0.clamp(0, last), (i0 + 1).clamp(0, last))
  };
  (a as usize, b as usize, s - s0)
}

/// Plane index of the nearest texel.
#[inline(always)]
fn nearest_at(src: &GatherSrc, u: f32, v: f32, wrap01: impl Fn(f32) -> f32 + Copy) -> usize {
  src.at(nearest_ix(u, src.w, wrap01), nearest_ix(v, src.h, wrap01))
}

/// Plane indices + weights of the four bilinear taps.
#[inline(always)]
fn bilinear_at(
  src: &GatherSrc,
  u: f32,
  v: f32,
  rep: bool,
  wrap01: impl Fn(f32) -> f32 + Copy,
) -> ([usize; 4], [f32; 4]) {
  let (x0, x1, fx) = linear_taps(u, src.w, rep, wrap01);
  let (y0, y1, fy) = linear_taps(v, src.h, rep, wrap01);
  let (gx, gy) = (1. - fx, 1. - fy);
  (
    [
      src.at(x0, y0),
      src.at(x1, y0),
      src.at(x0, y1),
      src.at(x1, y1),
    ],
    [gx * gy, fx * gy, gx * fy, fx * fy],
  )
}

#[inline(always)]
fn blend4(p: &[f32], k: &[usize; 4], w: &[f32; 4]) -> f32 {
  p[k[0]] * w[0] + p[k[1]] * w[1] + p[k[2]] * w[2] + p[k[3]] * w[3]
}

/// One texel of `src` at continuous `(u, v)`; `out[..planes.len()]` is written.
pub(crate) fn sample_texel(
  src: &GatherSrc,
  filter: SampleFilter,
  u: f32,
  v: f32,
  out: &mut [f32; 4],
) {
  with_wrap!(src.wrap, |f, rep| match filter {
    SampleFilter::Nearest => {
      let k = nearest_at(src, u, v, f);
      for (o, p) in out.iter_mut().zip(src.planes) {
        *o = p[k];
      }
    }
    SampleFilter::Bilinear => {
      let (k, w) = bilinear_at(src, u, v, rep, f);
      for (o, p) in out.iter_mut().zip(src.planes) {
        *o = blend4(p, &k, &w);
      }
    }
  })
}

/// Texels per address-resolve block: the resolved taps stay L1-resident while each plane
/// runs its own branch-free zip loop over them.
const BLK: usize = 64;

#[inline(always)]
fn gather_w(
  src: &GatherSrc,
  filter: SampleFilter,
  rep: bool,
  wrap01: impl Fn(f32) -> f32 + Copy,
  u: &[f32],
  v: &[f32],
  outs: &mut [Vec<f32>],
) {
  let n = u.len();
  match filter {
    SampleFilter::Nearest => {
      let mut ks = [0usize; BLK];
      for b in (0..n).step_by(BLK) {
        let m = (n - b).min(BLK);
        for (k, (&u, &v)) in ks[..m].iter_mut().zip(u[b..b + m].iter().zip(&v[b..b + m])) {
          *k = nearest_at(src, u, v, wrap01);
        }
        for (o, p) in outs.iter_mut().zip(src.planes) {
          for (o, &k) in o[b..b + m].iter_mut().zip(&ks[..m]) {
            *o = p[k];
          }
        }
      }
    }
    SampleFilter::Bilinear => {
      let mut taps = [([0usize; 4], [0f32; 4]); BLK];
      for b in (0..n).step_by(BLK) {
        let m = (n - b).min(BLK);
        for (t, (&u, &v)) in taps[..m]
          .iter_mut()
          .zip(u[b..b + m].iter().zip(&v[b..b + m]))
        {
          *t = bilinear_at(src, u, v, rep, wrap01);
        }
        for (o, p) in outs.iter_mut().zip(src.planes) {
          for (o, (k, w)) in o[b..b + m].iter_mut().zip(&taps[..m]) {
            *o = blend4(p, k, w);
          }
        }
      }
    }
  }
}

/// Whole-field gather: `outs[c][i] = src.planes[c]` sampled at `(u[i], v[i])`. Addresses
/// and weights resolve once per texel and feed every plane.
pub(crate) fn gather(
  src: &GatherSrc,
  filter: SampleFilter,
  u: &[f32],
  v: &[f32],
  outs: &mut [Vec<f32>],
) {
  let n = u.len();
  let v = &v[..n];
  for o in outs.iter_mut() {
    o.clear();
    o.resize(n, 0.);
  }
  with_wrap!(src.wrap, |f, rep| gather_w(src, filter, rep, f, u, v, outs))
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
