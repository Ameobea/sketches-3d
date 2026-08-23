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
    let mean = if n > 0 { sum / n as f64 } else { f64::NAN };
    let mut m2 = 0f64;
    for &x in plane {
      if x.is_finite() {
        m2 += (x as f64 - mean).powi(2);
      }
    }
    let std = if n > 0 {
      (m2 / n as f64).sqrt()
    } else {
      f64::NAN
    };

    // Stride both axes: a flat stride is a power of two on power-of-two planes and would
    // sample a handful of columns.
    let step = ((w * h).div_ceil(MAX_SAMPLE) as f64).sqrt().ceil().max(1.) as usize;
    let mut sample = Vec::with_capacity((w / step + 1) * (h / step + 1));
    for y in (0..h).step_by(step) {
      for x in (0..w).step_by(step) {
        let v = plane[y * w + x];
        if v.is_finite() {
          sample.push(v);
        }
      }
    }
    sample.sort_unstable_by(f32::total_cmp);

    ChannelStats {
      min: if n > 0 { min } else { f32::NAN },
      max: if n > 0 { max } else { f32::NAN },
      mean: mean as f32,
      std: std as f32,
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
