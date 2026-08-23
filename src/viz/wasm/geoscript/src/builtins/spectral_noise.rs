//! `spectral_noise`: synthesizes a seamless Gaussian noise texture from a compact
//! noise-signature fingerprint (param spec v1; see docs/noise-signature-plan.md and the
//! reference impl texture-synth-experiments/scripts/spec_v1.py, mirrored by the
//! texture-utils noise-signature extractor tool).

use std::f32::consts::PI;
use std::rc::Rc;

use fxhash::FxHashMap;
use rand::{RngExt, SeedableRng};
use rand_pcg::Pcg32;

use crate::{ArgRef, ErrorStack, EvalCtx, Mat4, Sym, TextureHandle, TextureWrap, Value};

const N_FIT: usize = 256;
const KR: usize = 8;
const KA: usize = 4;
const BAND_NATS: f32 = 14.;
const SIG_RANGE: (f32, f32) = (-3., -0.5);
const EN_RANGE: (f32, f32) = (-4., 2.);
const MAX_KERNELS: usize = 16;
const MAX_DIM: usize = 4096;

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
fn twiddles(len: usize, inverse: bool) -> Vec<[f32; 2]> {
  let sign = if inverse { 1.0f32 } else { -1.0 };
  let mut tw = Vec::with_capacity(len - 1);
  let mut half = 1usize;
  while half < len {
    let step = sign * PI / half as f32;
    for k in 0..half {
      let (s, c) = (step * k as f32).sin_cos();
      tw.push([c, s]);
    }
    half *= 2;
  }
  tw
}

/// In-place iterative radix-2 Cooley-Tukey over a contiguous row; `buf.len()` must be a
/// power of two and `tw` must come from `twiddles(buf.len(), ..)` for the matching
/// direction. Butterflies run as paired half-slices so the inner loop carries no bounds
/// checks or index arithmetic.
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

/// Both passes run over contiguous rows: the column pass transposes in and out rather than
/// striding, which at 1024² is worth far more than the two extra linear passes cost.
fn fft2d(buf: &mut [[f32; 2]], h: usize, w: usize, inverse: bool) {
  let tw_w = twiddles(w, inverse);
  for y in 0..h {
    fft1d(&mut buf[y * w..(y + 1) * w], &tw_w);
  }

  let mut scratch = vec![[0f32; 2]; w * h];
  transpose(buf, &mut scratch, h, w);
  let tw_h = if h == w { tw_w } else { twiddles(h, inverse) };
  for x in 0..w {
    fft1d(&mut scratch[x * h..(x + 1) * h], &tw_h);
  }
  transpose(&scratch, buf, w, h);

  if inverse {
    let s = 1. / (h * w) as f32;
    for v in buf.iter_mut() {
      v[0] *= s;
      v[1] *= s;
    }
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
      * (-x * x).exp();
  sign * y
}

fn band_centers() -> ([f32; KR], f32) {
  let c0 = (1. / N_FIT as f32).ln();
  let step = ((0.5f32).ln() - c0) / (KR - 1) as f32;
  (core::array::from_fn(|i| c0 + step * i as f32), step)
}

fn eval_bands_at(bands: &[[f32; KA]; KR], cs: &[f32; KR], step: f32, lnr: f32, th: f32) -> f32 {
  let lnr = lnr.clamp(cs[0], cs[KR - 1]);
  let ri = (((lnr - cs[0]) / step).floor().clamp(0., (KR - 2) as f32)) as usize;
  let t = ((lnr - cs[ri]) / step).clamp(0., 1.);
  let ap = th / PI * KA as f32 - 0.5;
  let a0 = (ap.floor() as isize).rem_euclid(KA as isize) as usize;
  let a1 = (a0 + 1) % KA;
  let at = ap - ap.floor();
  let gv = |i: usize, j: usize| bands[i][j];
  (gv(ri, a0) * (1. - t) * (1. - at)
    + gv(ri + 1, a0) * t * (1. - at)
    + gv(ri, a1) * (1. - t) * at
    + gv(ri + 1, a1) * t * at)
    .exp()
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

  // S(f) = bands(f / freq_scale) + kernel lobes; kernels scale with freq_scale too.
  // S depends only on |f| and the axis angle mod PI, and index negation maps f -> -f
  // exactly, so each evaluation fills its mirrored bin too and only half the grid is
  // actually evaluated.
  let (cs, step) = band_centers();
  let fxs: Vec<f32> = (0..w).map(|x| fftfreq(x, w) / freq_scale).collect();
  let fys: Vec<f32> = (0..h).map(|y| fftfreq(y, h) / freq_scale).collect();
  let mut spec = vec![0f32; w * h];
  let mut e_resid = 0f64;
  for y in 0..=h / 2 {
    let (fy, y2) = (fys[y], (h - y) % h);
    for x in 0..w {
      let x2 = (w - x) % w;
      // y2 == y is the DC/Nyquist row, whose mirror lands in the same row
      if (y2 == y && x2 < x) || (y == 0 && x == 0) {
        continue;
      }
      let fx = fxs[x];
      let r = (fx * fx + fy * fy).sqrt();
      let s = eval_bands_at(
        &bands,
        &cs,
        step,
        r.max(1e-9).ln(),
        fy.atan2(fx).rem_euclid(PI),
      );
      spec[y * w + x] = s;
      e_resid += s as f64;
      if (y2, x2) != (y, x) {
        spec[y2 * w + x2] = s;
        e_resid += s as f64;
      }
    }
  }

  for k in &kernels {
    let s1 = 10f32.powf(k.sig[0]);
    let s2 = 10f32.powf(k.sig[1]);
    let ang = k.angle;
    let ratio = 10f32.powf(k.energy);
    let (sa, ca) = ang.sin_cos();
    let i11 = ca * ca / (s1 * s1) + sa * sa / (s2 * s2);
    let i22 = sa * sa / (s1 * s1) + ca * ca / (s2 * s2);
    let i12 = ca * sa * (1. / (s1 * s1) - 1. / (s2 * s2));
    let mut lobe = vec![0f32; w * h];
    let mut lsum = 0f64;
    for y in 0..h {
      let fy = fftfreq(y, h) / freq_scale;
      for x in 0..w {
        if y == 0 && x == 0 {
          continue;
        }
        let fx = fftfreq(x, w) / freq_scale;
        let mut v = 0f32;
        for sgn in [1f32, -1.] {
          let dy = (fy - sgn * k.f0[0] + 0.5).rem_euclid(1.) - 0.5;
          let dx = (fx - sgn * k.f0[1] + 0.5).rem_euclid(1.) - 0.5;
          let q = i11 * dy * dy + 2. * i12 * dy * dx + i22 * dx * dx;
          if q < 40. {
            v += (-0.5 * q).exp();
          }
        }
        lobe[y * w + x] = v;
        lsum += v as f64;
      }
    }
    if lsum > 0. {
      let scale = (ratio as f64 * e_resid / lsum) as f32;
      for i in 0..w * h {
        spec[i] += lobe[i] * scale;
      }
    }
  }

  // Box-Muller, f32 and keeping BOTH outputs of each uniform pair: the transform is the
  // FFT's input white noise, so f64 precision and a discarded sine were pure cost.
  let mut rng = Pcg32::seed_from_u64((seed as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ 0x243F6A88);
  let mut buf: Vec<[f32; 2]> = Vec::with_capacity(w * h);
  while buf.len() < w * h {
    // `random::<f32>()` is uniform on [0, 1) with 24-bit granularity; flooring a zero draw
    // to one quantum keeps ln finite without inventing a tail sample the generator can't
    // otherwise produce.
    let u1 = rng.random::<f32>().max(1. / (1u32 << 24) as f32);
    let u2 = rng.random::<f32>();
    let r = (-2. * u1.ln()).sqrt();
    let (sin, cos) = (2. * PI * u2).sin_cos();
    buf.push([r * cos, 0.]);
    if buf.len() < w * h {
      buf.push([r * sin, 0.]);
    }
  }
  fft2d(&mut buf, h, w, false);
  for i in 0..w * h {
    let m = spec[i].sqrt();
    buf[i][0] *= m;
    buf[i][1] *= m;
  }
  buf[0] = [0., 0.];
  fft2d(&mut buf, h, w, true);

  let mut mean = 0f64;
  for v in &buf {
    mean += v[0] as f64;
  }
  mean /= (w * h) as f64;
  let mut var = 0f64;
  for v in &buf {
    var += (v[0] as f64 - mean).powi(2);
  }
  let std = (var / (w * h) as f64).sqrt().max(1e-12);

  let pixels: Vec<f32> = if uniform {
    buf
      .iter()
      .map(|v| 0.5 * (1. + erf(((v[0] as f64 - mean) / std) as f32 / std::f32::consts::SQRT_2)))
      .collect()
  } else {
    buf
      .iter()
      .map(|v| ((v[0] as f64 - mean) / std) as f32)
      .collect()
  };

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
}
