//! `spectral_noise`: synthesizes a seamless Gaussian noise texture from a compact
//! noise-signature fingerprint (param spec v1; see docs/noise-signature-plan.md and the
//! reference impl texture-synth-experiments/scripts/spec_v1.py, mirrored by the
//! texture-utils noise-signature extractor tool).
//!
//! The field is built straight in the frequency domain: a Hermitian half-plane spectrum
//! with Rayleigh magnitudes about `sqrt(S)` and uniform phase, then one complex-to-real
//! inverse FFT. That is the same random field as filtering white noise through `sqrt(S)`
//! — the forward transform only ever re-derived a spectrum whose distribution is known in
//! closed form — at a quarter of the butterflies and none of the whitening.

use std::f32::consts::{FRAC_1_SQRT_2, PI};
use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{ArgRef, ErrorStack, EvalCtx, Mat4, Sym, TextureHandle, TextureWrap, Value};

const N_FIT: usize = 256;
const KR: usize = 8;
const KA: usize = 4;
const BAND_NATS: f32 = 14.;
const SIG_RANGE: (f32, f32) = (-3., -0.5);
const EN_RANGE: (f32, f32) = (-4., 2.);
const MAX_KERNELS: usize = 16;
const MAX_DIM: usize = 4096;
/// Mahalanobis distance² past which a kernel lobe is treated as zero — also the ellipse
/// whose bounding box the lobe is evaluated over.
const LOBE_Q_MAX: f32 = 40.;

/// Polynomial replacements for the libm calls the spectrum builder makes per bin. Errors
/// are ~1e-5, which lands far below the fingerprint's own resolution; libm's `expf`/`logf`
/// were a third of the builtin's wasm runtime.
mod fm {
  /// `2^x`; ~3e-6 relative. `x` must stay inside ±120 so the exponent injection is valid.
  #[inline(always)]
  pub fn exp2(x: f32) -> f32 {
    let xc = x.clamp(-120., 120.);
    let n = xc.round_ties_even();
    let f = xc - n;
    let p = 0.999_999_26
      + f * (0.693_121_8 + f * (0.240_247_45 + f * (0.055_917_86 + f * 0.009_570_102)));
    f32::from_bits(((n as i32 + 127) as u32) << 23) * p
  }

  #[inline(always)]
  pub fn exp(x: f32) -> f32 {
    exp2(x * std::f32::consts::LOG2_E)
  }

  /// `log2` for finite `x > 0`; ~1.3e-5 absolute. Branch-free: the exponent comes off the
  /// bits and a degree-5 minimax covers the whole `[1, 2)` mantissa.
  #[inline(always)]
  pub fn log2(x: f32) -> f32 {
    let bits = x.to_bits();
    let e = ((bits >> 23) as i32) - 127;
    let m = f32::from_bits((bits & 0x007f_ffff) | 0x3f80_0000);
    e as f32
      + (-2.800_364
        + m
          * (5.091_710_8
            + m * (-3.550_793 + m * (1.631_148_8 + m * (-0.416_563_7 + m * 0.044_873_61)))))
  }

  #[inline(always)]
  pub fn ln(x: f32) -> f32 {
    log2(x) * std::f32::consts::LN_2
  }

  #[inline(always)]
  pub fn sin_cos_turns(u: f32) -> (f32, f32) {
    let x = u * 4.;
    let i = x as i32;
    let f = x - i as f32;
    let t = f * f;
    let s = f
      * (1.570_796_3
        + t * (-0.645_964_1 + t * (0.079_693_2 + t * (-0.004_681_75 + t * 0.000_160_44))));
    let c = 1.
      + t
        * (-1.233_700_5
          + t * (0.253_669_9 + t * (-0.020_864 + t * (0.000_919_2 + t * -0.000_025_2))));
    let (a, b) = if i & 1 != 0 { (c, s) } else { (s, c) };
    let flip = |v: f32, neg: bool| f32::from_bits(v.to_bits() ^ ((neg as u32) << 31));
    (flip(a, i & 2 != 0), flip(b, (i + 1) & 2 != 0))
  }
}

/// 4-wide forms of the same approximations, for the two per-bin loops big enough to carry
/// the setup. Lane-for-lane identical to `fm` (same coefficients, same operand order).
mod fm4 {
  use bytemuck::cast;
  use wide::{f32x4, i32x4, CmpEq, CmpLe, CmpLt};

  #[inline(always)]
  pub fn log2(x: f32x4) -> f32x4 {
    let bits: i32x4 = cast(x);
    let e: i32x4 = (bits >> 23) - i32x4::splat(127);
    let m: f32x4 = cast((bits & i32x4::splat(0x007f_ffff)) | i32x4::splat(0x3f80_0000));
    e.round_float()
      + (f32x4::splat(-2.800_364)
        + m
          * (f32x4::splat(5.091_710_8)
            + m
              * (f32x4::splat(-3.550_793)
                + m
                  * (f32x4::splat(1.631_148_8)
                    + m * (f32x4::splat(-0.416_563_7) + m * f32x4::splat(0.044_873_61))))))
  }

  #[inline(always)]
  pub fn ln(x: f32x4) -> f32x4 {
    log2(x) * f32x4::splat(std::f32::consts::LN_2)
  }

  #[inline(always)]
  pub fn exp2(x: f32x4) -> f32x4 {
    let xc = x.max(f32x4::splat(-120.)).min(f32x4::splat(120.));
    let ni = xc.round_int();
    let f = xc - ni.round_float();
    let p = f32x4::splat(0.999_999_26)
      + f
        * (f32x4::splat(0.693_121_8)
          + f
            * (f32x4::splat(0.240_247_45)
              + f * (f32x4::splat(0.055_917_86) + f * f32x4::splat(0.009_570_102))));
    cast::<i32x4, f32x4>((ni + i32x4::splat(127)) << 23) * p
  }

  #[inline(always)]
  pub fn atan_over_pi(z: f32x4) -> f32x4 {
    let t = z * z;
    z * (f32x4::splat(0.318_308_6)
      + t
        * (f32x4::splat(-0.105_858_08)
          + t
            * (f32x4::splat(0.060_485_67)
              + t * (f32x4::splat(-0.031_980_574) + t * f32x4::splat(0.009_092_633)))))
  }

  /// `(sin 2πu, cos 2πu)` for turn counts in `[0, 1)`.
  #[inline(always)]
  pub fn sin_cos_turns(u: f32x4) -> (f32x4, f32x4) {
    let x = u * f32x4::splat(4.);
    let i = x.trunc_int();
    let f = x - i.round_float();
    let t = f * f;
    let s = f
      * (f32x4::splat(1.570_796_3)
        + t
          * (f32x4::splat(-0.645_964_1)
            + t
              * (f32x4::splat(0.079_693_2)
                + t * (f32x4::splat(-0.004_681_75) + t * f32x4::splat(0.000_160_44)))));
    let c = f32x4::splat(1.)
      + t
        * (f32x4::splat(-1.233_700_5)
          + t
            * (f32x4::splat(0.253_669_9)
              + t
                * (f32x4::splat(-0.020_864)
                  + t * (f32x4::splat(0.000_919_2) + t * f32x4::splat(-0.000_025_2)))));
    let odd: f32x4 = cast((i & i32x4::splat(1)).cmp_eq(i32x4::splat(1)));
    let (a, b) = (odd.blend(c, s), odd.blend(s, c));
    let sign = |m: i32x4| -> f32x4 { cast(m & i32x4::splat(0x8000_0000u32 as i32)) };
    let neg_a = sign((i & i32x4::splat(2)).cmp_eq(i32x4::splat(2)));
    let neg_b = sign(((i + i32x4::splat(1)) & i32x4::splat(2)).cmp_eq(i32x4::splat(2)));
    (a ^ neg_a, b ^ neg_b)
  }

  /// `q` folded to `[0, 1)`: `|angle| / π` from the legs' ratio, then reflected by sign.
  #[inline(always)]
  pub fn angle_over_pi(fy: f32x4, ay: f32x4, ax: f32x4, neg_x: bool) -> f32x4 {
    let z = atan_over_pi(ay.min(ax) / ay.max(ax).max(f32x4::splat(f32::MIN_POSITIVE)));
    let le: f32x4 = cast(ay.cmp_le(ax));
    let q = le.blend(z, f32x4::splat(0.5) - z);
    let neg_y = fy.cmp_lt(f32x4::splat(0.));
    let flip = if neg_x {
      !cast::<f32x4, i32x4>(neg_y)
    } else {
      cast::<f32x4, i32x4>(neg_y)
    };
    cast::<i32x4, f32x4>(flip).blend(f32x4::splat(1.) - q, q)
  }
}

struct Kernel {
  f0: [f32; 2],
  sig: [f32; 2],
  angle: f32,
  energy: f32,
}

fn fftfreq(i: usize, n: usize) -> f32 {
  if i < n.div_ceil(2) {
    i as f32 / n as f32
  } else {
    (i as f32 - n as f32) / n as f32
  }
}

/// Per-stage twiddle factors for a `len`-point transform, flattened stage-major: stage
/// `half`'s k-th factor lives at `[half - 1 + k]` (stage sizes 1, 2, 4, … sum to `len - 1`).
/// Hoisting these out of the butterfly loop is what keeps `sin_cos` off the hot path —
/// one table serves every row and column of a 2D pass.
fn twiddles(len: usize) -> Vec<[f32; 2]> {
  let mut tw = Vec::with_capacity(len - 1);
  let mut half = 1usize;
  while half < len {
    let step = PI / half as f32;
    for k in 0..half {
      let (s, c) = (step * k as f32).sin_cos();
      tw.push([c, s]);
    }
    half *= 2;
  }
  tw
}

/// In-place iterative radix-2 Cooley-Tukey over a contiguous row; `buf.len()` must be a
/// power of two and `tw` must come from `twiddles(buf.len())`. Butterflies run as paired
/// half-slices so the inner loop carries no bounds checks or index arithmetic.
fn fft1d(buf: &mut [[f32; 2]], tw: &[[f32; 2]]) {
  let len = buf.len();
  let mut j = 0usize;
  for i in 1..len {
    let mut bit = len >> 1;
    while j & bit != 0 {
      j ^= bit;
      bit >>= 1;
    }
    j |= bit;
    if i < j {
      buf.swap(i, j);
    }
  }

  // Stage 0's twiddle is (1, 0), so it reduces to sum/difference.
  for pair in buf.chunks_exact_mut(2) {
    let ([ar, ai], [br, bi]) = (pair[0], pair[1]);
    pair[0] = [ar + br, ai + bi];
    pair[1] = [ar - br, ai - bi];
  }

  let mut half = 2usize;
  while half < len {
    let stage = &tw[half - 1..half * 2 - 1];
    for chunk in buf.chunks_exact_mut(half * 2) {
      let (lo, hi) = chunk.split_at_mut(half);
      for ((a, b), &[c, s]) in lo.iter_mut().zip(hi.iter_mut()).zip(stage) {
        let ([ar, ai], [br, bi]) = (*a, *b);
        let tr = br * c - bi * s;
        let ti = br * s + bi * c;
        *a = [ar + tr, ai + ti];
        *b = [ar - tr, ai - ti];
      }
    }
    half *= 2;
  }
}

/// `src` is `h` rows of `w`; `dst` becomes `w` rows of `h`. Tiled so both sides stay
/// within cache — a 1024² plane is 8 MB, so the naive version misses on every element.
fn transpose(src: &[[f32; 2]], dst: &mut [[f32; 2]], h: usize, w: usize) {
  const TILE: usize = 32;
  for y0 in (0..h).step_by(TILE) {
    let y1 = (y0 + TILE).min(h);
    for x0 in (0..w).step_by(TILE) {
      let x1 = (x0 + TILE).min(w);
      for y in y0..y1 {
        for x in x0..x1 {
          dst[x * h + y] = src[y * w + x];
        }
      }
    }
  }
}

/// One row of the inverse real transform. `spec` holds the `w/2 + 1` non-redundant bins of
/// a Hermitian spectrum; `out` receives `w` reals scaled by `scale`. Splitting the row into
/// its even/odd halves turns the length-`w` transform into a length-`w/2` complex one whose
/// real and imaginary parts *are* the interleaved output samples.
fn c2r_row(
  spec: &[[f32; 2]],
  out: &mut [f32],
  z: &mut [[f32; 2]],
  tw: &[[f32; 2]],
  unpack: &[[f32; 2]],
  scale: f32,
) {
  let half = z.len();
  for k in 0..half {
    let [ar, ai] = spec[k];
    let [br, bi] = spec[half - k];
    let (er, ei) = (0.5 * (ar + br), 0.5 * (ai - bi));
    let (dr, di) = (0.5 * (ar - br), 0.5 * (ai + bi));
    let [c, s] = unpack[k];
    z[k] = [er - (dr * s + di * c), ei + (dr * c - di * s)];
  }
  fft1d(z, tw);
  for (m, v) in z.iter().enumerate() {
    out[2 * m] = v[0] * scale;
    out[2 * m + 1] = v[1] * scale;
  }
}

fn erf(x: f32) -> f32 {
  // Abramowitz & Stegun 7.1.26
  let sign = if x < 0. { -1. } else { 1. };
  let x = x.abs();
  let t = 1. / (1. + 0.3275911 * x);
  let y = 1.
    - (((((1.061405429 * t - 1.453152027) * t) + 1.421413741) * t - 0.284496736) * t + 0.254829592)
      * t
      * fm::exp(-x * x);
  sign * y
}

fn band_centers() -> ([f32; KR], f32) {
  let c0 = (1. / N_FIT as f32).ln();
  let step = ((0.5f32).ln() - c0) / (KR - 1) as f32;
  (core::array::from_fn(|i| c0 + step * i as f32), step)
}

fn parse_f32_in(v: &Value, lo: f32, hi: f32, what: &str) -> Result<f32, ErrorStack> {
  let f = v
    .as_float()
    .ok_or_else(|| ErrorStack::new(format!("{what} must be numeric, found: {v:?}")))?;
  let tol = (hi - lo) * 1e-3;
  if f < lo - tol || f > hi + tol {
    return Err(ErrorStack::new(format!(
      "{what} out of range: {f}; expected [{lo}, {hi}]"
    )));
  }
  Ok(f.clamp(lo, hi))
}

fn parse_bands(ctx: &EvalCtx, v: &Value) -> Result<[[f32; KA]; KR], ErrorStack> {
  let seq = v
    .as_sequence()
    .ok_or_else(|| ErrorStack::new(format!("`bands` must be a sequence, found: {v:?}")))?;
  let mut bands = [[0f32; KA]; KR];
  let mut n_rows = 0usize;
  for (i, row) in seq.consume(ctx).enumerate() {
    let row = row?;
    if i >= KR {
      return Err(ErrorStack::new(format!(
        "`bands` must have exactly {KR} rows"
      )));
    }
    let row_seq = row.as_sequence().ok_or_else(|| {
      ErrorStack::new(format!(
        "`bands` row {i} must be a sequence of {KA} numbers, found: {row:?}"
      ))
    })?;
    let mut n_cols = 0usize;
    for (j, cell) in row_seq.consume(ctx).enumerate() {
      let cell = cell?;
      if j >= KA {
        return Err(ErrorStack::new(format!(
          "`bands` row {i} must have exactly {KA} entries"
        )));
      }
      bands[i][j] = parse_f32_in(
        &cell,
        -BAND_NATS,
        0.,
        &format!("`bands`[{i}][{j}] (log-gain, nats)"),
      )?;
      n_cols += 1;
    }
    if n_cols != KA {
      return Err(ErrorStack::new(format!(
        "`bands` row {i} has {n_cols} entries; expected {KA}"
      )));
    }
    n_rows += 1;
  }
  if n_rows != KR {
    return Err(ErrorStack::new(format!(
      "`bands` has {n_rows} rows; expected {KR}"
    )));
  }
  Ok(bands)
}

fn parse_kernels(ctx: &EvalCtx, v: &Value) -> Result<Vec<Kernel>, ErrorStack> {
  if v.is_nil() {
    return Ok(Vec::new());
  }
  let seq = v
    .as_sequence()
    .ok_or_else(|| ErrorStack::new(format!("`kernels` must be a sequence, found: {v:?}")))?;
  let mut kernels = Vec::new();
  for (i, k) in seq.consume(ctx).enumerate() {
    let k = k?;
    if i >= MAX_KERNELS {
      return Err(ErrorStack::new(format!(
        "`kernels` supports at most {MAX_KERNELS} entries"
      )));
    }
    let kseq = k.as_sequence().ok_or_else(|| {
      ErrorStack::new(format!(
        "`kernels`[{i}] must be a sequence [f0y, f0x, sig1, sig2, angle, energy], found: {k:?}"
      ))
    })?;
    let vals: Vec<Value> = kseq.consume(ctx).collect::<Result<_, _>>()?;
    if vals.len() != 6 {
      return Err(ErrorStack::new(format!(
        "`kernels`[{i}] has {} entries; expected 6: [f0y, f0x, sig1, sig2, angle, energy]",
        vals.len()
      )));
    }
    let f0y = vals[0].as_float().ok_or_else(|| {
      ErrorStack::new(format!(
        "`kernels`[{i}][0] (f0y) must be numeric, found: {:?}",
        vals[0]
      ))
    })?;
    let f0x = vals[1].as_float().ok_or_else(|| {
      ErrorStack::new(format!(
        "`kernels`[{i}][1] (f0x) must be numeric, found: {:?}",
        vals[1]
      ))
    })?;
    if !(-0.5..=0.5).contains(&f0y) || !(-0.5..=0.5).contains(&f0x) {
      return Err(ErrorStack::new(format!(
        "`kernels`[{i}] center frequency ({f0y}, {f0x}) out of range; expected cycles/pixel in \
         [-0.5, 0.5]"
      )));
    }
    kernels.push(Kernel {
      f0: [f0y, f0x],
      sig: [
        parse_f32_in(
          &vals[2],
          SIG_RANGE.0,
          SIG_RANGE.1,
          &format!("`kernels`[{i}][2] (sig1, log10)"),
        )?,
        parse_f32_in(
          &vals[3],
          SIG_RANGE.0,
          SIG_RANGE.1,
          &format!("`kernels`[{i}][3] (sig2, log10)"),
        )?,
      ],
      angle: parse_f32_in(
        &vals[4],
        0.,
        PI,
        &format!("`kernels`[{i}][4] (angle, radians)"),
      )?,
      energy: parse_f32_in(
        &vals[5],
        EN_RANGE.0,
        EN_RANGE.1,
        &format!("`kernels`[{i}][5] (energy, log10)"),
      )?,
    });
  }
  Ok(kernels)
}

/// One kernel lobe restricted to the index box of its `LOBE_Q_MAX` ellipse. `ys`/`xs` carry
/// the surviving bin indices with their wrapped frequency offsets; `vals` is the dense
/// `ys × xs` block of `exp(-q/2)`.
struct LobeBox {
  ys: Vec<(usize, f32)>,
  xs: Vec<(usize, f32)>,
  vals: Vec<f32>,
}

/// Bilinear lookup on the (log-radius, angle) fingerprint grid. `p` is the fractional
/// radius index, `u` the angle as a fraction of a half turn; `bl` is pre-scaled to log2 so
/// the caller's exponential is a bare `exp2`.
#[inline(always)]
fn band_gain_log2(bl: &[[f32; KA]; KR], p: f32, u: f32) -> f32 {
  let ri = (p as usize).min(KR - 2);
  let t = p - ri as f32;
  let ap = u * KA as f32 - 0.5;
  let af = ap.floor();
  let at = ap - af;
  let a0 = ((af as i32 + KA as i32) & (KA as i32 - 1)) as usize;
  let a1 = (a0 + 1) & (KA - 1);
  let (r0, r1) = (&bl[ri], &bl[ri + 1]);
  let g0 = r0[a0] + (r0[a1] - r0[a0]) * at;
  let g1 = r1[a0] + (r1[a1] - r1[a0]) * at;
  g0 + (g1 - g0) * t
}

/// Band spectrum down one column of the half-plane. Run as three passes over a column-sized
/// scratch rather than one: `log2`, the table lookup and `exp2` are each a long serial
/// dependency chain, and separating them is what lets consecutive bins overlap.
fn band_column(
  bl: &[[f32; KA]; KR],
  out: &mut [f32],
  angs: &mut [f32],
  fx: f32,
  fys: &[f32],
  ra: f32,
  rb: f32,
) {
  use wide::f32x4;
  let (ax, fx2) = (fx.abs(), fx * fx);
  let neg_x = fx < 0.;
  let v4 = |c: &[f32]| f32x4::from(<[f32; 4]>::try_from(c).unwrap());
  let (axv, fx2v) = (f32x4::splat(ax), f32x4::splat(fx2));
  let (rav, rbv) = (f32x4::splat(ra), f32x4::splat(rb));
  let phi = f32x4::splat((KR - 1) as f32);
  for ((o, a), f) in out
    .chunks_exact_mut(4)
    .zip(angs.chunks_exact_mut(4))
    .zip(fys.chunks_exact(4))
  {
    let fy = v4(f);
    let p = (fm4::log2(fx2v + fy * fy) * rav + rbv)
      .max(f32x4::ZERO)
      .min(phi);
    o.copy_from_slice(&p.to_array());
    a.copy_from_slice(&fm4::angle_over_pi(fy, fy.abs(), axv, neg_x).to_array());
  }
  for (o, &a) in out.iter_mut().zip(angs.iter()) {
    *o = band_gain_log2(bl, *o, a);
  }
  for o in out.chunks_exact_mut(4) {
    o.copy_from_slice(&fm4::exp2(v4(o)).to_array());
  }
}

/// PCG32 (XSH-RR) run as four state chains advanced by `M^4`, which yields exactly the
/// sequence a single `Pcg32` would — read four at a time — with four independent multiply
/// chains instead of one. The fill is otherwise latency-bound on a single 64-bit LCG step.
struct Rng4 {
  s: [u64; 4],
  inc4: u64,
}

const PCG_MULT: u64 = 6364136223846793005;
const PCG_INC: u64 = 1442695040888963407;

impl Rng4 {
  fn new(seed: u64) -> Self {
    let mut st = seed.wrapping_add(PCG_INC);
    let s = core::array::from_fn(|_| {
      let cur = st;
      st = st.wrapping_mul(PCG_MULT).wrapping_add(PCG_INC);
      cur
    });
    let m2 = PCG_MULT.wrapping_mul(PCG_MULT);
    let sum = PCG_MULT
      .wrapping_mul(m2)
      .wrapping_add(m2)
      .wrapping_add(PCG_MULT)
      .wrapping_add(1);
    Rng4 {
      s,
      inc4: PCG_INC.wrapping_mul(sum),
    }
  }

  /// Four uniforms on `[0, 1)` with 24-bit granularity.
  #[inline(always)]
  fn next4(&mut self) -> [f32; 4] {
    let m4 = {
      let m2 = PCG_MULT.wrapping_mul(PCG_MULT);
      m2.wrapping_mul(m2)
    };
    core::array::from_fn(|i| {
      let st = self.s[i];
      self.s[i] = st.wrapping_mul(m4).wrapping_add(self.inc4);
      let xsh = ((((st >> 18) ^ st) >> 27) as u32).rotate_right((st >> 59) as u32);
      (xsh >> 8) as f32 * (1. / (1u32 << 24) as f32)
    })
  }
}

/// Spectrum bins for one column: magnitude Rayleigh about `sqrt(s)`, phase uniform, from
/// two uniform streams. Returns the summed squared magnitude, which Parseval needs anyway —
/// so standardizing the field costs no extra pass over it.
fn draw_bins(s: &[f32], ua: &[f32], ub: &[f32], out: &mut [[f32; 2]]) -> f64 {
  use wide::f32x4;
  // `random::<f32>()` is uniform on [0, 1) with 24-bit granularity; flooring a zero draw to
  // one quantum keeps ln finite without inventing a tail sample the generator can't produce.
  let min_u = f32x4::splat(1. / (1u32 << 24) as f32);
  let v4 = |c: &[f32]| f32x4::from(<[f32; 4]>::try_from(c).unwrap());
  let n = out.len() & !3;
  let mut acc = f32x4::ZERO;
  for (i, o) in out[..n].chunks_exact_mut(4).enumerate() {
    let k = i * 4;
    // the approximate log crosses zero a hair either side of u = 1, so clamp before the root
    let m2 = (v4(&s[k..k + 4]) * -fm4::ln(v4(&ua[k..k + 4]).max(min_u))).max(f32x4::ZERO);
    let (sn, cs) = fm4::sin_cos_turns(v4(&ub[k..k + 4]));
    let r = m2.sqrt();
    let (re, im) = ((r * cs).to_array(), (r * sn).to_array());
    for (j, slot) in o.iter_mut().enumerate() {
      *slot = [re[j], im[j]];
    }
    acc += m2;
  }
  let mut total = f64::from(acc.reduce_add());
  for (i, o) in out[n..].iter_mut().enumerate() {
    let m2 = (s[n + i] * -fm::ln(ua[n + i].max(1. / (1u32 << 24) as f32))).max(0.);
    let r = m2.sqrt();
    let (sn, cs) = fm::sin_cos_turns(ub[n + i]);
    *o = [r * cs, r * sn];
    total += m2 as f64;
  }
  total
}

pub(crate) fn spectral_noise_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let bands = parse_bands(ctx, arg_refs[0].resolve(args, kwargs))?;
  let kernels = parse_kernels(ctx, arg_refs[1].resolve(args, kwargs))?;
  let width = arg_refs[2].resolve(args, kwargs).as_int().unwrap();
  let height = arg_refs[3].resolve(args, kwargs).as_int().unwrap();
  let seed = arg_refs[4].resolve(args, kwargs).as_int().unwrap();
  let freq_scale = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
  let uniform = match arg_refs[6].resolve(args, kwargs).as_str().unwrap() {
    "gaussian" => false,
    "uniform" => true,
    other => {
      return Err(ErrorStack::new(format!(
        "`distribution` must be \"gaussian\" or \"uniform\"; found {other:?}"
      )))
    }
  };

  for (dim, name) in [(width, "width"), (height, "height")] {
    if dim < 4 || dim > MAX_DIM as i64 || !(dim as u64).is_power_of_two() {
      return Err(ErrorStack::new(format!(
        "`{name}` must be a power of two in 4..={MAX_DIM} (FFT synthesis); found {dim}"
      )));
    }
  }
  if !(0.125..=8.).contains(&freq_scale) {
    return Err(ErrorStack::new(format!(
      "`freq_scale` must be in [0.125, 8]; found {freq_scale}"
    )));
  }
  let (w, h) = (width as usize, height as usize);
  // Non-redundant spectrum width: the field is real, so bins past w/2 are the conjugates of
  // bins before it. Held column-major (kx outer) so the ky transform runs over contiguous
  // memory and the whole synthesis needs one transpose.
  let hw = w / 2 + 1;

  // S(f) = bands(f / freq_scale) + kernel lobes; kernels scale with freq_scale too.
  let bl = bands.map(|row| row.map(|v| v * std::f32::consts::LOG2_E));
  let (cs, step) = band_centers();
  let (ra, rb) = (0.5 * std::f32::consts::LN_2 / step, -cs[0] / step);
  let fxs: Vec<f32> = (0..w).map(|x| fftfreq(x, w) / freq_scale).collect();
  let fys: Vec<f32> = (0..h).map(|y| fftfreq(y, h) / freq_scale).collect();

  // Kernel energies are quoted relative to the band residual, so with kernels present the
  // band pass has to complete (and be kept) before any lobe can be scaled. Without them the
  // spectrum is consumed one column at a time and never materialized.
  let mut angs = vec![0f32; h];
  let mut spec = Vec::new();
  if !kernels.is_empty() {
    spec = vec![0f32; hw * h];
    let mut e_resid = 0f64;
    for kx in 0..hw {
      let col = &mut spec[kx * h..(kx + 1) * h];
      band_column(&bl, col, &mut angs, fxs[kx], &fys, ra, rb);
      if kx == 0 {
        col[0] = 0.;
      }
      let (mut a, mut b) = (0f64, 0f64);
      for p in col.chunks_exact(2) {
        a += p[0] as f64;
        b += p[1] as f64;
      }
      // the two self-conjugate columns already hold both halves of each mirrored pair
      e_resid += (a + b) * if kx == 0 || kx == w / 2 { 1. } else { 2. };
    }
    add_kernel_lobes(&kernels, &mut spec, &fxs, &fys, w, h, hw, e_resid);
  }

  // |F| ~ Rayleigh(sqrt(S)) with uniform phase — the distribution the forward transform of
  // white noise would have produced. Only the columns that map to themselves under bin
  // negation carry a symmetry constraint; the rest are free. Each column is transformed the
  // moment it is filled, so it is still in cache.
  let mut rng = Rng4::new((seed as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0x243F6A88);
  let mut buf: Vec<[f32; 2]> = vec![[0.; 2]; hw * h];
  let tw_h = twiddles(h);
  let mut s_col = vec![0f32; h];
  let (mut ua, mut ub) = (vec![0f32; h], vec![0f32; h]);
  let mut half_col = vec![[0f32; 2]; h / 2 + 1];
  let mut power = 0f64;
  for kx in 0..hw {
    let s: &[f32] = if spec.is_empty() {
      band_column(&bl, &mut s_col, &mut angs, fxs[kx], &fys, ra, rb);
      if kx == 0 {
        s_col[0] = 0.;
      }
      &s_col
    } else {
      &spec[kx * h..(kx + 1) * h]
    };
    for (a, b) in ua.chunks_exact_mut(4).zip(ub.chunks_exact_mut(4)) {
      a.copy_from_slice(&rng.next4());
      b.copy_from_slice(&rng.next4());
    }
    let col = &mut buf[kx * h..(kx + 1) * h];
    if kx == 0 || kx == w / 2 {
      // this column maps to itself under bin negation: ky and h - ky are a conjugate pair,
      // and the two fixed points have to be real
      draw_bins(&s[..h / 2 + 1], &ua, &ub, &mut half_col);
      for ky in 1..h / 2 {
        let z = half_col[ky];
        col[ky] = z;
        col[h - ky] = [z[0], -z[1]];
        power += 2. * (z[0] * z[0] + z[1] * z[1]) as f64;
      }
      for ky in [0usize, h / 2] {
        // a real bin's Box-Muller partner is the phase it already drew
        let v = if kx == 0 && ky == 0 {
          0.
        } else {
          half_col[ky][0] * std::f32::consts::SQRT_2
        };
        col[ky] = [v, 0.];
        power += (v * v) as f64;
      }
    } else {
      power += 2. * draw_bins(s, &ua, &ub, col);
    }
    fft1d(col, &tw_h);
  }
  drop(spec);

  // The synthesis emits (w·h / 2)·f, so standardizing to unit variance folds into one
  // constant — Parseval reads the variance off the spectrum, no pass over the field.
  let scale = (2. / power.max(1e-30).sqrt()) as f32;
  let mut rows = vec![[0f32; 2]; hw * h];
  transpose(&buf, &mut rows, hw, h);
  drop(buf);

  let half = w / 2;
  let tw_half = twiddles(half);
  let unpack: Vec<[f32; 2]> = (0..half)
    .map(|k| {
      let (s, c) = fm::sin_cos_turns(k as f32 / w as f32);
      [c, s]
    })
    .collect();
  let mut z = vec![[0f32; 2]; half];
  let mut pixels = vec![0f32; w * h];
  for (y, out) in pixels.chunks_exact_mut(w).enumerate() {
    c2r_row(
      &rows[y * hw..(y + 1) * hw],
      out,
      &mut z,
      &tw_half,
      &unpack,
      scale,
    );
  }

  if uniform {
    for p in &mut pixels {
      *p = 0.5 * (1. + erf(*p * FRAC_1_SQRT_2));
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: crate::TexStorage::planes(vec![Rc::new(pixels)]),
    width: w,
    height: h,
    channels: 1,
    wrap: TextureWrap::Repeat,
    min_filter: None,
    mag_filter: None,
    format: None,
    transform: Mat4::identity(),
    mips: Default::default(),
  })))
}

/// Adds each kernel's oriented Gaussian lobe into `spec`, scaled so the lobe carries
/// `10^energy` times the band residual. Lobes are compactly supported, so only the bins
/// inside the `LOBE_Q_MAX` ellipse's bounding box are visited.
#[allow(clippy::too_many_arguments)]
fn add_kernel_lobes(
  kernels: &[Kernel],
  spec: &mut [f32],
  fxs: &[f32],
  fys: &[f32],
  w: usize,
  h: usize,
  hw: usize,
  e_resid: f64,
) {
  for k in kernels {
    let (s1, s2) = (10f32.powf(k.sig[0]), 10f32.powf(k.sig[1]));
    let (sa, ca) = k.angle.sin_cos();
    let (is1, is2) = (1. / (s1 * s1), 1. / (s2 * s2));
    let i11 = ca * ca * is1 + sa * sa * is2;
    let i22 = sa * sa * is1 + ca * ca * is2;
    let i12 = ca * sa * (is1 - is2);
    // half-extents of the q = LOBE_Q_MAX ellipse's bounding box, from the inverse form
    let ry = (LOBE_Q_MAX * (sa * sa * s2 * s2 + ca * ca * s1 * s1)).sqrt();
    let rx = (LOBE_Q_MAX * (ca * ca * s2 * s2 + sa * sa * s1 * s1)).sqrt();

    let mut lsum = 0f64;
    let mut boxes = Vec::with_capacity(2);
    for sgn in [1f32, -1.] {
      let pick = |f: f32, c: f32| (f - sgn * c + 0.5).rem_euclid(1.) - 0.5;
      let ys: Vec<(usize, f32)> = (0..h)
        .filter_map(|y| {
          let d = pick(fys[y], k.f0[0]);
          (d.abs() <= ry).then_some((y, d))
        })
        .collect();
      // only stored bins: the mirrored half's lobe value equals its representative's, and
      // each `sgn` supplies one of the two terms there
      let xs: Vec<(usize, f32)> = (0..hw)
        .filter_map(|x| {
          let d = pick(fxs[x], k.f0[1]);
          (d.abs() <= rx).then_some((x, d))
        })
        .collect();
      let mut vals = vec![0f32; ys.len() * xs.len()];
      for (iy, &(y, dy)) in ys.iter().enumerate() {
        let (qy, cross) = (i11 * dy * dy, 2. * i12 * dy);
        for (ix, &(x, dx)) in xs.iter().enumerate() {
          if y == 0 && x == 0 {
            continue;
          }
          let q = qy + cross * dx + i22 * dx * dx;
          if q < LOBE_Q_MAX {
            let v = fm::exp(-0.5 * q);
            vals[iy * xs.len() + ix] = v;
            lsum += (v * if x == 0 || x == w / 2 { 1. } else { 2. }) as f64;
          }
        }
      }
      boxes.push(LobeBox { ys, xs, vals });
    }

    if lsum > 0. {
      let scale = (10f64.powf(k.energy as f64) * e_resid / lsum) as f32;
      for b in &boxes {
        let nx = b.xs.len();
        for (iy, &(y, _)) in b.ys.iter().enumerate() {
          for (ix, &(x, _)) in b.xs.iter().enumerate() {
            spec[x * h + y] += b.vals[iy * nx + ix] * scale;
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::parse_and_eval_program;

  const FIXTURE: &str = r#"
t = spectral_noise(
  bands=[
    [-0.842, -0.421, 0.000, -0.421],
    [-4.365, -2.608, -2.704, -3.001],
    [-5.312, -3.539, -3.799, -4.824],
    [-5.927, -5.180, -4.673, -5.768],
    [-6.050, -6.288, -5.318, -5.654],
    [-6.022, -6.586, -5.198, -5.497],
    [-5.999, -7.028, -5.494, -5.508],
    [-6.620, -7.527, -6.478, -6.229]
  ],
  kernels=[
    [0.142230, -0.000659, -2.034, -2.193, 0.425, -0.838],
    [0.106623, -0.001861, -1.400, -2.115, 0.008, -0.866],
    [0.226508, -0.009847, -1.238, -1.832, 0.174, -0.639],
    [0.157495, 0.016886, -1.867, -2.470, 3.129, -2.092]
  ],
  seed=7
)
t | render_texture(name="field")
"#;

  #[test]
  fn spectral_noise_basic() {
    let ctx = parse_and_eval_program(FIXTURE).unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    let tex = &rendered[0].texture;
    assert_eq!((tex.width, tex.height, tex.channels), (256, 256, 1));
    let n = tex.as_interleaved().len() as f64;
    let mean = tex.as_interleaved().iter().map(|&v| v as f64).sum::<f64>() / n;
    let var = tex
      .as_interleaved()
      .iter()
      .map(|&v| (v as f64 - mean).powi(2))
      .sum::<f64>()
      / n;
    assert!(mean.abs() < 0.01, "mean {mean}");
    assert!((var.sqrt() - 1.).abs() < 0.01, "std {}", var.sqrt());

    let ctx = parse_and_eval_program(
      "spectral_noise(bands=[[-3,-3,-3,-3],[-3.5,-3.5,-3.5,-3.5],[-4.1,-4.1,-4.1,-4.1],[-4.7,-4.7,\
       -4.7,-4.7],[-5.2,-5.2,-5.2,-5.2],[-5.8,-5.8,-5.8,-5.8],[-6.3,-6.3,-6.3,-6.3],[-6.9,-6.9,-6.\
       9,-6.9]], width=64, height=64, distribution=\"uniform\") | render_texture",
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    let tex = &rendered[0].texture;
    let mean = tex.as_interleaved().iter().map(|&v| v as f64).sum::<f64>()
      / tex.as_interleaved().len() as f64;
    assert!((mean - 0.5).abs() < 0.02, "uniform mean {mean}");
    assert!(tex
      .as_interleaved()
      .iter()
      .all(|&v| (0. ..=1.).contains(&v)));
  }

  #[test]
  fn spectral_noise_validation() {
    let err = parse_and_eval_program("spectral_noise(bands=[[-1,-2,-3,-4]])").unwrap_err();
    assert!(err.to_string().contains("expected 8"), "{err}");

    // old u8-format fingerprints fail loudly
    let err = parse_and_eval_program(
      "spectral_noise(bands=[[240,204,255,209],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],\
       [0,0,0,0],[0,0,0,0]])",
    )
    .unwrap_err();
    assert!(err.to_string().contains("out of range"), "{err}");

    let err = parse_and_eval_program(
      "spectral_noise(bands=[[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,\
       0],[0,0,0,0]], width=300)",
    )
    .unwrap_err();
    assert!(err.to_string().contains("power of two"), "{err}");
  }

  #[test]
  fn spectral_noise_rect_and_seed_determinism() {
    let src = |seed: u32| {
      format!(
        "spectral_noise(bands=[[-3,-3,-3,-3],[-3.5,-3.5,-3.5,-3.5],[-4.1,-4.1,-4.1,-4.1],[-4.7,-4.\
         7,-4.7,-4.7],[-5.2,-5.2,-5.2,-5.2],[-5.8,-5.8,-5.8,-5.8],[-6.3,-6.3,-6.3,-6.3],[-6.9,-6.\
         9,-6.9,-6.9]], width=64, height=32, seed={seed}) | render_texture"
      )
    };
    let px = |seed: u32| {
      let ctx = parse_and_eval_program(&src(seed)).unwrap();
      let rendered = ctx.rendered_textures.into_inner();
      assert_eq!(
        (rendered[0].texture.width, rendered[0].texture.height),
        (64, 32)
      );
      rendered[0].texture.as_interleaved()
    };
    let a = px(1);
    let b = px(1);
    let c = px(2);
    assert_eq!(a, b);
    assert_ne!(a, c);
  }

  /// The variation-stack idiom end to end: equal-power seed morph, ramped per layer,
  /// published as a stack.
  #[test]
  fn spectral_morph_stack() {
    let ctx = parse_and_eval_program(
      r#"
bands = [[-3,-3,-3,-3],[-3.5,-3.5,-3.5,-3.5],[-4.1,-4.1,-4.1,-4.1],[-4.7,-4.7,-4.7,-4.7],[-5.2,-5.2,-5.2,-5.2],[-5.8,-5.8,-5.8,-5.8],[-6.3,-6.3,-6.3,-6.3],[-6.9,-6.9,-6.9,-6.9]]
f0 = spectral_noise(bands=bands, width=32, height=32, seed=1)
f1 = spectral_noise(bands=bands, width=32, height=32, seed=2)
ramp = color_ramp(stops=[srgb(0x202020), srgb(0xf0f0f0)], domain=[-2.5, 2.5])
layers = 0..9 -> |i| ramp(cos(i / 8. * 2. * pi) * f0 + sin(i / 8. * 2. * pi) * f1)
layers | render_texture_stack(name="morph", usage="albedo")
"#,
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 1);
    assert_eq!(rendered[0].extra_slices.len(), 8);
    assert_eq!(rendered[0].texture.channels, 3);
    assert_eq!(
      (rendered[0].texture.width, rendered[0].texture.height),
      (32, 32)
    );
  }

  #[test]
  fn dbg_sizes() {
    for n in [64usize, 128, 256, 512, 1024] {
      let src = format!(
        "spectral_noise(bands=[[-3,-3,-3,-3],[-3.5,-3.5,-3.5,-3.5],[-4.1,-4.1,-4.1,-4.1],[-4.7,-4.\
         7,-4.7,-4.7],[-5.2,-5.2,-5.2,-5.2],[-5.8,-5.8,-5.8,-5.8],[-6.3,-6.3,-6.3,-6.3],[-6.9,-6.\
         9,-6.9,-6.9]], width={n}, height={n}) | render_texture"
      );
      let ctx = crate::parse_and_eval_program(&src).unwrap();
      let r = ctx.rendered_textures.into_inner();
      let px = r[0].texture.as_interleaved();
      let nan = px.iter().position(|v| !v.is_finite());
      println!(
        "n={n} first_nonfinite={:?} v0={} count_nonfinite={}",
        nan,
        px[0],
        px.iter().filter(|v| !v.is_finite()).count()
      );
    }
  }
}
