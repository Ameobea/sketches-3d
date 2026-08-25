//! Per-channel value statistics of a texture, computed lazily and cached on its storage.

use crate::TextureHandle;

const MAX_SAMPLE: usize = 65536;
/// Quantile table length in the host wire: q_i = quantile(i / 256), so q_0/q_256 = min/max.
pub const WIRE_QUANTILES: usize = 257;

pub struct ChannelStats {
  pub min: f32,
  pub max: f32,
  pub mean: f32,
  pub std: f32,
  /// NaN/±inf texels; excluded from every other field.
  pub nonfinite: u32,
  /// Sorted stride-sample of finite texels (the whole plane when it has ≤ 64k texels).
  sample: Vec<f32>,
}

impl ChannelStats {
  fn compute(plane: &[f32], w: usize, h: usize) -> Self {
    let (min, max, mean, std, nonfinite) = moments(plane);

    // Stride both axes: a flat stride is a power of two on power-of-two planes and would
    // sample a handful of columns.
    let step = ((w * h).div_ceil(MAX_SAMPLE) as f64).sqrt().ceil().max(1.) as usize;
    let mut keys = Vec::with_capacity((w / step + 1) * (h / step + 1));
    for y in (0..h).step_by(step) {
      let row = &plane[y * w..(y + 1) * w];
      for x in (0..w).step_by(step) {
        let v = row[x];
        if v.is_finite() {
          keys.push(sort_key(v));
        }
      }
    }
    // `f32::total_cmp` is a handful of bit ops per *comparison*; folding it into the key up
    // front leaves a plain integer sort, and at 64k values a radix beats the comparison sort.
    radix_sort(&mut keys);
    let sample = keys.into_iter().map(from_sort_key).collect();

    ChannelStats {
      min,
      max,
      mean,
      std,
      nonfinite,
      sample,
    }
  }

  /// Value at quantile `q` ∈ [0, 1], lerped between order statistics of the sample; 0 and 1
  /// return the exact min/max.
  pub fn quantile(&self, q: f32) -> f32 {
    let s = &self.sample;
    if s.is_empty() {
      return f32::NAN;
    }
    if q <= 0. {
      return self.min;
    }
    if q >= 1. {
      return self.max;
    }
    let pos = q as f64 * (s.len() - 1) as f64;
    let i = pos as usize;
    let (a, b) = (s[i], s[(i + 1).min(s.len() - 1)]);
    a + (b - a) * (pos - i as f64) as f32
  }

  /// Empirical CDF in [0, 1]: mid-rank on ties, lerped between neighbouring samples
  /// otherwise. Non-finite `x` passes through clamped (NaN stays NaN).
  pub fn cdf(&self, x: f32) -> f32 {
    let s = &self.sample;
    let n = s.len();
    if !x.is_finite() || n == 0 {
      return x.clamp(0., 1.);
    }
    let denom = (n - 1).max(1) as f32;
    let lo = s.partition_point(|&v| v < x);
    let hi = s.partition_point(|&v| v <= x);
    if lo < hi {
      return (lo + hi - 1) as f32 * 0.5 / denom;
    }
    if lo == 0 {
      return 0.;
    }
    if lo == n {
      return 1.;
    }
    let (a, b) = (s[lo - 1], s[lo]);
    ((lo - 1) as f32 + (x - a) / (b - a)) / denom
  }
}

/// `f32` reinterpreted so that unsigned integer order matches `f32::total_cmp` order.
#[inline(always)]
fn sort_key(v: f32) -> u32 {
  let b = v.to_bits();
  b ^ (((b as i32 >> 31) as u32) | 0x8000_0000)
}

#[inline(always)]
fn from_sort_key(k: u32) -> f32 {
  f32::from_bits(k ^ ((((k ^ 0x8000_0000) as i32) >> 31) as u32 | 0x8000_0000))
}

/// LSD radix sort, one byte per pass, skipping any byte that is constant across the input —
/// texture samples usually share an exponent range, so the high passes are typically free.
fn radix_sort(keys: &mut Vec<u32>) {
  let n = keys.len();
  if n < 2 {
    return;
  }
  let mut hist = [[0u32; 256]; 4];
  for &k in keys.iter() {
    for (d, h) in hist.iter_mut().enumerate() {
      h[(k >> (8 * d)) as u8 as usize] += 1;
    }
  }
  let mut scratch = vec![0u32; n];
  let (mut src, mut dst) = (&mut keys[..], &mut scratch[..]);
  let mut swapped = false;
  for (d, h) in hist.iter().enumerate() {
    if h.iter().any(|&c| c as usize == n) {
      continue;
    }
    let mut base = 0u32;
    let mut offs = [0u32; 256];
    for (o, &c) in offs.iter_mut().zip(h.iter()) {
      *o = base;
      base += c;
    }
    for &k in src.iter() {
      let b = (k >> (8 * d)) as u8 as usize;
      dst[offs[b] as usize] = k;
      offs[b] += 1;
    }
    std::mem::swap(&mut src, &mut dst);
    swapped = !swapped;
  }
  if swapped {
    keys.copy_from_slice(&scratch);
  }
}

/// `(min, max, mean, std, nonfinite)`. The common all-finite case runs as one branch-free
/// pass — sums are taken about a shifted origin so a single pass can't lose the variance to
/// cancellation, and f32 block sums are flushed to f64 often enough to stay accurate. Any
/// non-finite texel poisons the sums, which is exactly the signal to rerun the filtered
/// version.
fn moments(plane: &[f32]) -> (f32, f32, f32, f32, u32) {
  const BLOCK: usize = 4096;
  let shift = plane.iter().copied().find(|v| v.is_finite()).unwrap_or(0.);
  let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
  let (mut s1, mut s2) = (0f64, 0f64);
  for block in plane.chunks(BLOCK) {
    let (b1, b2, bmin, bmax) = block_moments(block, shift);
    s1 += b1 as f64;
    s2 += b2 as f64;
    lo = lo.min(bmin);
    hi = hi.max(bmax);
  }

  let n = plane.len() as f64;
  if s1.is_finite() && s2.is_finite() && lo.is_finite() && hi.is_finite() {
    let m = s1 / n;
    let var = (s2 / n - m * m).max(0.);
    return (lo, hi, (m + shift as f64) as f32, var.sqrt() as f32, 0);
  }
  moments_filtered(plane)
}

/// `(sum, sum of squares, min, max)` of one cache-sized block, the sums about `shift`.
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline]
fn block_moments(block: &[f32], shift: f32) -> (f32, f32, f32, f32) {
  use core::arch::wasm32::*;
  let sh = f32x4_splat(shift);
  let (mut a, mut b) = (f32x4_splat(0.), f32x4_splat(0.));
  let (mut mn, mut mx) = (f32x4_splat(f32::INFINITY), f32x4_splat(f32::NEG_INFINITY));
  let mut it = block.chunks_exact(4);
  for c in &mut it {
    let x = unsafe { v128_load(c.as_ptr() as *const v128) };
    let d = f32x4_sub(x, sh);
    a = f32x4_add(a, d);
    b = f32x4_add(b, f32x4_mul(d, d));
    mn = f32x4_pmin(mn, x);
    mx = f32x4_pmax(mx, x);
  }
  let red = |v: v128, f: fn(f32, f32) -> f32| {
    f(
      f(f32x4_extract_lane::<0>(v), f32x4_extract_lane::<1>(v)),
      f(f32x4_extract_lane::<2>(v), f32x4_extract_lane::<3>(v)),
    )
  };
  let (mut sa, mut sb) = (red(a, |x, y| x + y), red(b, |x, y| x + y));
  let (mut lo, mut hi) = (red(mn, f32::min), red(mx, f32::max));
  for &v in it.remainder() {
    let d = v - shift;
    sa += d;
    sb += d * d;
    lo = lo.min(v);
    hi = hi.max(v);
  }
  (sa, sb, lo, hi)
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
#[inline]
fn block_moments(block: &[f32], shift: f32) -> (f32, f32, f32, f32) {
  let (mut a, mut b) = ([0f32; 4], [0f32; 4]);
  let (mut mn, mut mx) = ([f32::INFINITY; 4], [f32::NEG_INFINITY; 4]);
  let mut it = block.chunks_exact(4);
  for c in &mut it {
    for i in 0..4 {
      let d = c[i] - shift;
      a[i] += d;
      b[i] += d * d;
      mn[i] = if c[i] < mn[i] { c[i] } else { mn[i] };
      mx[i] = if c[i] > mx[i] { c[i] } else { mx[i] };
    }
  }
  let (mut sa, mut sb) = (a[0] + a[1] + a[2] + a[3], b[0] + b[1] + b[2] + b[3]);
  let (mut lo, mut hi) = (
    mn[0].min(mn[1]).min(mn[2].min(mn[3])),
    mx[0].max(mx[1]).max(mx[2].max(mx[3])),
  );
  for &v in it.remainder() {
    let d = v - shift;
    sa += d;
    sb += d * d;
    lo = lo.min(v);
    hi = hi.max(v);
  }
  (sa, sb, lo, hi)
}

/// Two-pass reference path, used once a plane is known to hold NaN or ±inf.
#[cold]
fn moments_filtered(plane: &[f32]) -> (f32, f32, f32, f32, u32) {
  let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
  let (mut sum, mut n, mut nonfinite) = (0f64, 0u64, 0u32);
  for &x in plane {
    if x.is_finite() {
      min = min.min(x);
      max = max.max(x);
      sum += x as f64;
      n += 1;
    } else {
      nonfinite += 1;
    }
  }
  if n == 0 {
    return (f32::NAN, f32::NAN, f32::NAN, f32::NAN, nonfinite);
  }
  let mean = sum / n as f64;
  let mut m2 = 0f64;
  for &x in plane {
    if x.is_finite() {
      m2 += (x as f64 - mean).powi(2);
    }
  }
  (
    min,
    max,
    mean as f32,
    (m2 / n as f64).sqrt() as f32,
    nonfinite,
  )
}

pub struct TexStats {
  pub channels: Vec<ChannelStats>,
}

impl TexStats {
  pub(crate) fn compute(t: &TextureHandle) -> Self {
    TexStats {
      channels: t
        .as_planes()
        .iter()
        .map(|p| ChannelStats::compute(p, t.width, t.height))
        .collect(),
    }
  }

  /// Flat host table, per channel: `[min, max, mean, std, nonfinite, q_0 … q_256]`.
  /// Non-finite entries are zeroed so the table survives JSON.
  pub fn to_wire(&self) -> Vec<f32> {
    let fin = |v: f32| if v.is_finite() { v } else { 0. };
    let mut out = Vec::with_capacity(self.channels.len() * (5 + WIRE_QUANTILES));
    for c in &self.channels {
      out.extend_from_slice(&[
        fin(c.min),
        fin(c.max),
        fin(c.mean),
        fin(c.std),
        c.nonfinite as f32,
      ]);
      out.extend(
        (0..WIRE_QUANTILES).map(|i| fin(c.quantile(i as f32 / (WIRE_QUANTILES - 1) as f32))),
      );
    }
    out
  }
}

#[cfg(test)]
mod tests {
  use crate::parse_and_eval_program;

  fn tex(src: &str) -> std::rc::Rc<crate::TextureHandle> {
    let ctx = parse_and_eval_program(src).unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    std::rc::Rc::clone(&rendered[0].texture)
  }

  #[test]
  fn stats_match_brute_force_and_views() {
    // 8x8 ramp in x, scaled per channel, with one NaN texel in channel 0.
    let t = tex(
      "t = texture(8, 8, |uv, x_ix, y_ix| if x_ix == 3 && y_ix == 2 { v3(sqrt(-1.), uv.x * 2., \
       5.) } else { v3(uv.x, uv.x * 2., 5.) })\nt | render_texture",
    );
    let s = t.stats();
    let c0 = &s.channels[0];
    let xs: Vec<f32> = (0..8).map(|x| (x as f32 + 0.5) / 8.).collect();
    assert_eq!(c0.nonfinite, 1);
    assert_eq!((c0.min, c0.max), (xs[0], xs[7]));
    let mean = (xs.iter().sum::<f32>() * 8. - xs[3]) / 63.;
    assert!((c0.mean - mean).abs() < 1e-6, "{} vs {mean}", c0.mean);
    assert!((c0.quantile(0.5) - 0.5).abs() < 0.07);
    assert!((s.channels[1].std - 2. * c0.std).abs() < 0.02);
    assert_eq!(
      (s.channels[2].min, s.channels[2].max, s.channels[2].std),
      (5., 5., 0.)
    );
    assert_eq!((c0.cdf(-1.), c0.cdf(10.)), (0., 1.));
    // 63 finite samples; each column is 8 ties → mid-rank of the band.
    assert!(
      (c0.cdf(xs[0]) - 3.5 / 62.).abs() < 1e-6,
      "{}",
      c0.cdf(xs[0])
    );
    assert!(
      (c0.cdf(xs[7]) - 58.5 / 62.).abs() < 1e-6,
      "{}",
      c0.cdf(xs[7])
    );
    assert!((c0.cdf(0.5 * (xs[3] + xs[4])) - 0.5).abs() < 0.03);

    // A crop view carries its own stats over the view region only.
    let v = tex("t = texture(8, 8, |uv| uv.x)\nt[0.., 4..] | render_texture");
    let vs = v.stats();
    assert_eq!((v.width, v.height), (4, 8));
    assert_eq!((vs.channels[0].min, vs.channels[0].max), (xs[4], xs[7]));
  }

  /// A full-width ramp on a power-of-two plane must sample across both axes, not a
  /// handful of columns — the quantiles then track the ramp.
  #[test]
  fn sample_covers_both_axes() {
    let t = tex("t = texture(2048, 2048, |uv| uv.x)\nt | render_texture");
    let c = &t.stats().channels[0];
    for q in [0.1f32, 0.5, 0.9] {
      assert!((c.quantile(q) - q).abs() < 0.01, "q{q} = {}", c.quantile(q));
    }
    assert_eq!(t.stats().to_wire().len(), 5 + super::WIRE_QUANTILES);
  }
}
