//! Image-processing builtins: `resize`, `dilate`/`erode`, `concat_channels`, levels.

use std::rc::Rc;

use fxhash::FxHashMap;

use super::texture::{
  blur_tex, premultiplied_planes, resolve_tex_range, texture_map_chan, texture_zip,
  unpremultiply_planes, MAX_TEXTURE_DIM,
};
use crate::{ArgRef, ErrorStack, Sym, TexStorage, TextureHandle, TextureWrap, Value};

#[derive(Clone, Copy, PartialEq)]
enum ResizeFilter {
  Nearest,
  Box,
  Triangle,
  Mitchell,
  Lanczos3,
}

impl ResizeFilter {
  fn from_name(s: &str) -> Result<Self, ErrorStack> {
    match s {
      "nearest" => Ok(Self::Nearest),
      "box" => Ok(Self::Box),
      "triangle" => Ok(Self::Triangle),
      "mitchell" => Ok(Self::Mitchell),
      "lanczos3" => Ok(Self::Lanczos3),
      _ => Err(ErrorStack::new(format!(
        "Invalid resize filter: \"{s}\"; expected one of \"nearest\", \"box\", \"triangle\", \
         \"mitchell\", \"lanczos3\""
      ))),
    }
  }

  fn support(self) -> f32 {
    match self {
      Self::Nearest => 0.5,
      Self::Box => 0.5,
      Self::Triangle => 1.,
      Self::Mitchell => 2.,
      Self::Lanczos3 => 3.,
    }
  }

  fn eval(self, x: f32) -> f32 {
    let x = x.abs();
    match self {
      Self::Nearest | Self::Box => {
        if x <= 0.5 {
          1.
        } else {
          0.
        }
      }
      Self::Triangle => (1. - x).max(0.),
      Self::Mitchell => {
        // B = C = 1/3
        const B: f32 = 1. / 3.;
        const C: f32 = 1. / 3.;
        if x < 1. {
          ((12. - 9. * B - 6. * C) * x * x * x
            + (-18. + 12. * B + 6. * C) * x * x
            + (6. - 2. * B))
            / 6.
        } else if x < 2. {
          ((-B - 6. * C) * x * x * x
            + (6. * B + 30. * C) * x * x
            + (-12. * B - 48. * C) * x
            + (8. * B + 24. * C))
            / 6.
        } else {
          0.
        }
      }
      Self::Lanczos3 => {
        if x < 1e-6 {
          1.
        } else if x < 3. {
          let pix = std::f32::consts::PI * x;
          3. * pix.sin() * (pix / 3.).sin() / (pix * pix)
        } else {
          0.
        }
      }
    }
  }
}

/// Per-output-pixel taps: (first source index, weights). Kernel support scales with the
/// minification ratio so downsampling is area-correct (stb_image_resize approach).
fn build_weights(in_len: usize, out_len: usize, filter: ResizeFilter) -> Vec<(i64, Vec<f32>)> {
  let scale = out_len as f32 / in_len as f32;
  let (kernel_scale, support) = if scale < 1. {
    (scale, filter.support() / scale)
  } else {
    (1., filter.support())
  };
  (0..out_len)
    .map(|o| {
      let center = (o as f32 + 0.5) / scale - 0.5;
      if filter == ResizeFilter::Nearest {
        return (center.round() as i64, vec![1.]);
      }
      let lo = (center - support).ceil() as i64;
      let hi = (center + support).floor() as i64;
      let mut weights: Vec<f32> = (lo..=hi)
        .map(|i| filter.eval((i as f32 - center) * kernel_scale))
        .collect();
      let sum: f32 = weights.iter().sum();
      if sum.abs() > 1e-8 {
        for w in &mut weights {
          *w /= sum;
        }
      }
      (lo, weights)
    })
    .collect()
}

/// Source offsets are resolved once per (output position, tap), not per texel: the wrap is
/// invariant across the other axis and the planes, and dominates the multiply-add it
/// guards. The resolved taps are then replayed for every plane.
fn resize_pass(
  planes: &[Vec<f32>],
  sw: usize,
  sh: usize,
  wrap: TextureWrap,
  out_len: usize,
  horizontal: bool,
  filter: ResizeFilter,
) -> Vec<Vec<f32>> {
  let (in_len, other) = if horizontal { (sw, sh) } else { (sh, sw) };
  let (ow, oh) = if horizontal {
    (out_len, other)
  } else {
    (other, out_len)
  };
  // Stride between consecutive taps, and between consecutive `fixed` positions.
  let (tap_stride, fixed_stride) = if horizontal { (1, sw) } else { (sw, 1) };
  let taps: Vec<(Vec<usize>, Vec<f32>)> = build_weights(in_len, out_len, filter)
    .into_iter()
    .map(|(lo, weights)| {
      let offsets = (0..weights.len())
        .map(|i| wrap.coord(lo + i as i64, in_len) * tap_stride)
        .collect();
      (offsets, weights)
    })
    .collect();

  planes
    .iter()
    .map(|px| {
      let mut out = vec![0f32; ow * oh];
      for y in 0..oh {
        for x in 0..ow {
          let (pos, fixed) = if horizontal { (x, y) } else { (y, x) };
          let (offsets, weights) = &taps[pos];
          let base = y * ow + x;
          let fixed_base = fixed * fixed_stride;
          for (&off, &w) in offsets.iter().zip(weights) {
            out[base] += w * px[fixed_base + off];
          }
        }
      }
      out
    })
    .collect()
}

pub(crate) fn resize_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let width = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
  let height = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
  let tex = arg_refs[2].resolve(args, kwargs).as_texture().unwrap();
  let filter = ResizeFilter::from_name(arg_refs[3].resolve(args, kwargs).as_str().unwrap())?;
  if width < 1 || height < 1 || width > MAX_TEXTURE_DIM || height > MAX_TEXTURE_DIM {
    return Err(ErrorStack::new(format!(
      "Invalid resize dims {width}x{height}; expected 1..={MAX_TEXTURE_DIM} per side"
    )));
  }
  let (w, h) = (width as usize, height as usize);
  let ch = tex.channels;

  let src = premultiplied_planes(tex);
  let mid = resize_pass(&src, tex.width, tex.height, tex.wrap, w, true, filter);
  let mut out = resize_pass(&mid, w, tex.height, tex.wrap, h, false, filter);
  if ch == 4 {
    unpremultiply_planes(&mut out);
  }
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::from_plane_vecs(out),
    width: w,
    height: h,
    mips: Default::default(),
    ..(**tex).clone()
  })))
}

/// van Herk–Gil-Werman running extreme over a padded line: `src.len() == out.len() + 2r`,
/// window `2r+1`; O(1) per pixel at any radius.
fn vhgw_line(src: &[f32], out: &mut [f32], r: usize, dilate: bool, g: &mut Vec<f32>, h: &mut Vec<f32>) {
  let w = 2 * r + 1;
  let len = src.len();
  let ext = if dilate { f32::max } else { f32::min };
  g.clear();
  g.extend_from_slice(src);
  for i in 1..len {
    if i % w != 0 {
      g[i] = ext(g[i - 1], src[i]);
    }
  }
  h.clear();
  h.extend_from_slice(src);
  for i in (0..len - 1).rev() {
    if (i + 1) % w != 0 {
      h[i] = ext(h[i + 1], src[i]);
    }
  }
  for (j, o) in out.iter_mut().enumerate() {
    *o = ext(h[j], g[j + w - 1]);
  }
}

/// `r == 0` is an identity pass (window width 1), so callers composing morphology don't
/// need to special-case it.
fn morph_pass(dilate: bool, r: usize, tex: &TextureHandle) -> TextureHandle {
  let (w, h) = (tex.width, tex.height);
  let wrap = tex.wrap;

  let mut planes: Vec<Vec<f32>> = tex.as_planes().iter().map(|p| p.to_vec()).collect();
  let (mut g, mut hbuf) = (Vec::new(), Vec::new());
  let mut padded = vec![0f32; 0];
  let mut line_out = vec![0f32; 0];
  for horizontal in [true, false] {
    let (n, other) = if horizontal { (w, h) } else { (h, w) };
    padded.resize(n + 2 * r, 0.);
    line_out.resize(n, 0.);
    // Only the `r` entries at each end of the line can wrap, so the interior strides the
    // plane directly instead of paying rem_euclid per element (same split `blur_tex`'s
    // `pass` uses).
    let stride = if horizontal { 1 } else { w };
    for px in &mut planes {
      let mut out = vec![0f32; w * h];
      let tap = |px: &[f32], x: i64, y: i64| px[wrap.coord(y, h) * w + wrap.coord(x, w)];
      for o in 0..other {
        let o_base = if horizontal { o * w } else { o };
        for i in 0..n {
          padded[r + i] = px[o_base + i * stride];
        }
        for i in 0..r {
          let (lo, hi) = (i as i64 - r as i64, (n + i) as i64);
          let (a, b) = if horizontal {
            (tap(px, lo, o as i64), tap(px, hi, o as i64))
          } else {
            (tap(px, o as i64, lo), tap(px, o as i64, hi))
          };
          padded[i] = a;
          padded[r + n + i] = b;
        }
        vhgw_line(&padded, &mut line_out, r, dilate, &mut g, &mut hbuf);
        for (i, &v) in line_out.iter().enumerate() {
          let (x, y) = if horizontal { (i, o) } else { (o, i) };
          out[y * w + x] = v;
        }
      }
      *px = out;
    }
  }
  TextureHandle {
    storage: TexStorage::from_plane_vecs(planes),
    mips: Default::default(),
    ..tex.clone()
  }
}

fn morph_radius(name: &str, radius: i64) -> Result<usize, ErrorStack> {
  if radius > MAX_TEXTURE_DIM {
    return Err(ErrorStack::new(format!(
      "`{name}` radius {radius} too large; max {MAX_TEXTURE_DIM}"
    )));
  }
  Ok(radius.max(0) as usize)
}

pub(crate) fn dilate_erode_impl(
  dilate: bool,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let radius = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
  let tex_val = arg_refs[1].resolve(args, kwargs);
  if radius <= 0 {
    return Ok(tex_val.clone());
  }
  let r = morph_radius(if dilate { "dilate" } else { "erode" }, radius)?;
  Ok(Value::Texture(Rc::new(morph_pass(
    dilate,
    r,
    tex_val.as_texture().unwrap(),
  ))))
}

#[derive(Clone, Copy)]
pub(crate) enum MorphOp {
  Open,
  Close,
  Outline,
  Tophat,
  Blackhat,
}

impl MorphOp {
  fn name(self) -> &'static str {
    match self {
      Self::Open => "morph_open",
      Self::Close => "morph_close",
      Self::Outline => "morph_outline",
      Self::Tophat => "morph_tophat",
      Self::Blackhat => "morph_blackhat",
    }
  }
}

pub(crate) fn morph_composite_impl(
  op: MorphOp,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let radius = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
  let tex = arg_refs[1].resolve(args, kwargs).as_texture().unwrap();
  let r = morph_radius(op.name(), radius)?;
  let open_close = |open: bool| morph_pass(open, r, &morph_pass(!open, r, tex));
  let sub = |a: &TextureHandle, b: &TextureHandle| texture_zip(a, b, op.name(), |x, y| x - y);
  match op {
    MorphOp::Open => Ok(Value::Texture(Rc::new(open_close(true)))),
    MorphOp::Close => Ok(Value::Texture(Rc::new(open_close(false)))),
    MorphOp::Outline => sub(&morph_pass(true, r, tex), &morph_pass(false, r, tex)),
    MorphOp::Tophat => sub(tex, &open_close(true)),
    MorphOp::Blackhat => sub(&open_close(false), tex),
  }
}

pub(crate) fn crop_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let i = |ix: usize| arg_refs[ix].resolve(args, kwargs).as_int().unwrap();
  let (x, y, w, h) = (i(0), i(1), i(2), i(3));
  let tex = arg_refs[4].resolve(args, kwargs).as_texture().unwrap();
  let (x0, w) = resolve_tex_range(x, Some(x.saturating_add(w)), tex.width, "col")?;
  let (y0, h) = resolve_tex_range(y, Some(y.saturating_add(h)), tex.height, "row")?;
  Ok(Value::Texture(Rc::new(tex.crop_view(x0, y0, w, h))))
}

pub(crate) fn sharpen_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let tex_val = arg_refs[0].resolve(args, kwargs);
  let tex = tex_val.as_texture().unwrap();
  let amt = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
  let sigma = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
  if sigma <= 0. || amt == 0. {
    return Ok(tex_val.clone());
  }
  let blurred = blur_tex(sigma, tex);
  texture_zip(tex, &blurred, "sharpen", |x, b| x + (x - b) * amt)
}

/// `sigmas`: nil → the exact [min, max] window; `k` → mean ± k·std; `[lo, hi]` → signed
/// z-positions around the mean. Output is clamped to [0, 1]; a constant channel maps to 0.
pub(crate) fn texture_normalize_impl(
  ctx: &crate::EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let t = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let err = || {
    ErrorStack::new(
      "texture_normalize: `sigmas` must be a positive number or a `[lo, hi]` pair (or vec2) \
       of z-positions with lo < hi",
    )
  };
  let z = match arg_refs[1].resolve(args, kwargs) {
    Value::Nil => None,
    Value::Vec2(v) => Some((v.x, v.y)),
    Value::Sequence(seq) => {
      let parts: Vec<Value> = seq.consume(ctx).collect::<Result<_, _>>()?;
      if parts.len() != 2 {
        return Err(err());
      }
      Some((
        parts[0].as_float().ok_or_else(err)?,
        parts[1].as_float().ok_or_else(err)?,
      ))
    }
    v => {
      let k = v.as_float().ok_or_else(err)?;
      Some((-k, k))
    }
  };
  if let Some((lo, hi)) = z {
    if !(lo < hi) {
      return Err(err());
    }
  }

  let stats = t.stats();
  let (mut lo, mut scale) = ([0f32; 4], [0f32; 4]);
  for c in 0..t.channels {
    let s = &stats.channels[c];
    let (a, b) = match z {
      None => (s.min, s.max),
      Some((zl, zh)) => (s.mean + zl * s.std, s.mean + zh * s.std),
    };
    lo[c] = a;
    scale[c] = 1. / (b - a).max(1e-8);
  }
  Ok(texture_map_chan(t, |x, c| ((x - lo[c]) * scale[c]).clamp(0., 1.)))
}

pub(crate) fn texture_standardize_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let t = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let stats = t.stats();
  let (mut mean, mut inv) = ([0f32; 4], [0f32; 4]);
  for c in 0..t.channels {
    let s = &stats.channels[c];
    mean[c] = s.mean;
    inv[c] = if s.std > 1e-12 { 1. / s.std } else { 0. };
  }
  Ok(texture_map_chan(t, |x, c| (x - mean[c]) * inv[c]))
}

pub(crate) fn texture_equalize_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let t = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let stats = t.stats();
  Ok(texture_map_chan(t, |x, c| stats.channels[c].cdf(x)))
}

enum ChannelSrc<'a> {
  Tex(&'a TextureHandle),
  Const(f32),
}

/// Materializing by design: a view indexes one dense base, so a multi-source read can't be
/// a view.
pub(crate) fn concat_channels_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let mut srcs = Vec::with_capacity(arg_refs.len());
  let mut first_tex: Option<&TextureHandle> = None;
  let mut total_ch = 0usize;
  for arg_ref in arg_refs {
    match arg_ref.resolve(args, kwargs) {
      Value::Texture(t) => {
        match first_tex {
          Some(f) if (f.width, f.height) != (t.width, t.height) => {
            return Err(ErrorStack::new(format!(
              "`concat_channels` requires matching dims; found {}x{} vs {}x{}",
              f.width, f.height, t.width, t.height
            )))
          }
          Some(_) => (),
          None => first_tex = Some(t),
        }
        total_ch += t.channels;
        srcs.push(ChannelSrc::Tex(t));
      }
      other => {
        total_ch += 1;
        srcs.push(ChannelSrc::Const(other.as_float().unwrap()));
      }
    }
  }

  let Some(t0) = first_tex else {
    return Err(ErrorStack::new(
      "`concat_channels` requires at least one texture argument to determine output dims",
    ));
  };
  if total_ch > 4 {
    return Err(ErrorStack::new(format!(
      "`concat_channels` produced {total_ch} channels; textures hold at most 4"
    )));
  }

  // The SoA payoff: texture sources contribute their planes by Rc clone — zero copy.
  let (w, h) = (t0.width, t0.height);
  let mut planes: Vec<Rc<Vec<f32>>> = Vec::with_capacity(total_ch);
  for src in &srcs {
    match src {
      ChannelSrc::Tex(t) => planes.extend(t.as_planes()),
      ChannelSrc::Const(v) => planes.push(Rc::new(vec![*v; w * h])),
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::planes(planes),
    channels: total_ch,
    mips: Default::default(),
    ..t0.clone()
  })))
}

#[derive(Clone, Copy)]
pub(crate) struct LevelsParams {
  pub in_lo: f32,
  pub in_hi: f32,
  pub out_lo: f32,
  pub out_hi: f32,
  pub gamma: f32,
}

pub(crate) const IDENTITY_LEVELS: LevelsParams = LevelsParams {
  in_lo: 0.,
  in_hi: 1.,
  out_lo: 0.,
  out_hi: 1.,
  gamma: 1.,
};

pub(crate) const LEVELS_KEYS: [&str; 5] = ["in_lo", "in_hi", "out_lo", "out_hi", "gamma"];

impl LevelsParams {
  pub(crate) fn as_array(&self) -> [f32; 5] {
    [self.in_lo, self.in_hi, self.out_lo, self.out_hi, self.gamma]
  }

  /// Missing keys fall back to identity values.
  pub(crate) fn from_map(m: &FxHashMap<String, Value>) -> LevelsParams {
    let d = IDENTITY_LEVELS;
    let g = |k: &str, d: f32| m.get(k).and_then(|v| v.as_float()).unwrap_or(d);
    LevelsParams {
      in_lo: g("in_lo", d.in_lo),
      in_hi: g("in_hi", d.in_hi),
      out_lo: g("out_lo", d.out_lo),
      out_hi: g("out_hi", d.out_hi),
      gamma: g("gamma", d.gamma),
    }
  }

  fn from_array(v: [f32; 5]) -> LevelsParams {
    LevelsParams { in_lo: v[0], in_hi: v[1], out_lo: v[2], out_hi: v[3], gamma: v[4] }
  }

  pub(crate) fn to_map_value(self) -> Value {
    let mut m = FxHashMap::default();
    for (k, v) in LEVELS_KEYS.iter().zip(self.as_array()) {
      m.insert((*k).to_owned(), Value::Float(v));
    }
    Value::Map(Rc::new(m))
  }
}

/// The only two places the levels wire key order is spelled out, so the host crate never
/// open-codes it (mirrors the `ramp_*` pair).
pub fn image_levels_value_from_wire(vals: &[f32]) -> Option<Value> {
  let v: [f32; 5] = vals.get(..5)?.try_into().ok()?;
  Some(LevelsParams::from_array(v).to_map_value())
}

pub fn image_levels_control_value(v: &Value) -> Option<Vec<f32>> {
  match v {
    Value::Map(m) => Some(LevelsParams::from_map(m).as_array().to_vec()),
    _ => None,
  }
}

/// `out = out_lo + (out_hi − out_lo) · clamp((x − in_lo) / (in_hi − in_lo), 0, 1)^(1/gamma)`
/// applied to color channels; alpha is preserved on 4-channel textures.
pub(crate) fn apply_levels(t: &Rc<TextureHandle>, p: LevelsParams) -> Value {
  if p.as_array() == IDENTITY_LEVELS.as_array() {
    return Value::Texture(Rc::clone(t));
  }
  let inv_gamma = 1. / p.gamma.max(1e-4);
  let in_range = (p.in_hi - p.in_lo).abs().max(1e-8) * (p.in_hi - p.in_lo).signum();
  // libm doesn't special-case exponent 1, and gamma is untouched in most levels edits.
  let map1 = |x: f32| {
    let t = ((x - p.in_lo) / in_range).clamp(0., 1.);
    let t = if inv_gamma == 1. { t } else { t.powf(inv_gamma) };
    p.out_lo + (p.out_hi - p.out_lo) * t
  };
  let alpha_ix = if t.channels == 4 { 3 } else { usize::MAX };
  texture_map_chan(t, |x, c| if c == alpha_ix { x } else { map1(x) })
}

pub(crate) fn texture_levels_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let f = |ix: usize| arg_refs[ix].resolve(args, kwargs).as_float().unwrap();
  let p = LevelsParams {
    in_lo: f(0),
    in_hi: f(1),
    out_lo: f(2),
    out_hi: f(3),
    gamma: f(4),
  };
  let tex = arg_refs[5].resolve(args, kwargs).as_texture().unwrap();
  Ok(apply_levels(tex, p))
}

pub(crate) fn input_image_levels_impl(
  ctx: &crate::EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let c = super::input_common(ctx, arg_refs, args, kwargs, 3)?;
  let has_override = c.injected.is_some();
  let tex = arg_refs[1].resolve(args, kwargs).as_texture().unwrap();

  let injected = c.injected.as_ref().and_then(|v| match v {
    Value::Map(m) => Some(LevelsParams::from_map(m)),
    _ => None,
  });
  let params = match injected {
    Some(p) => p,
    None => match arg_refs[2].resolve(args, kwargs) {
      Value::Nil => IDENTITY_LEVELS,
      // Author-supplied, so a typo'd key would otherwise silently yield identity.
      Value::Map(m) => {
        for (k, v) in m.iter() {
          if !LEVELS_KEYS.contains(&k.as_str()) {
            return Err(ErrorStack::new(format!(
              "input_image_levels: unknown `default` key `{k}`; expected any of {LEVELS_KEYS:?}"
            )));
          }
          if v.as_float().is_none() {
            return Err(ErrorStack::new(format!(
              "input_image_levels: `default.{k}` must be a number, got {v:?}"
            )));
          }
        }
        LevelsParams::from_map(m)
      }
      other => {
        return Err(ErrorStack::new(format!(
          "input_image_levels: `default` must be nil or a map with keys {LEVELS_KEYS:?}, got \
           {other:?}"
        )))
      }
    },
  };

  ctx.rendered_controls.push(crate::RenderedControl {
    source_module: c.module,
    handle_id: c.handle_id,
    kind: crate::ControlKind::ImageLevels,
    label: c.label,
    current_value: params.to_map_value(),
    min: None,
    max: None,
    step: None,
    style: None,
    options: Vec::new(),
    stats: Some(tex.stats()),
    has_override,
  });
  Ok(apply_levels(tex, params))
}

#[cfg(test)]
mod tests {
  use crate::{parse_and_eval_program, TextureHandle, Value};
  use std::rc::Rc;

  fn get_tex(ctx: &crate::EvalCtx, name: &str) -> Rc<TextureHandle> {
    match ctx.get_global(name).unwrap() {
      Value::Texture(t) => t,
      other => panic!("Expected {name} to be a texture, found: {other:?}"),
    }
  }

  /// The normalize family end to end on a standardized Gaussian field (64² = exact stats).
  #[test]
  fn normalize_family() {
    let ctx = parse_and_eval_program(
      r#"
n = spectral_noise(bands=[[-3,-3,-3,-3],[-3.5,-3.5,-3.5,-3.5],[-4.1,-4.1,-4.1,-4.1],[-4.7,-4.7,-4.7,-4.7],[-5.2,-5.2,-5.2,-5.2],[-5.8,-5.8,-5.8,-5.8],[-6.3,-6.3,-6.3,-6.3],[-6.9,-6.9,-6.9,-6.9]], width=64, height=64)
z = (n * 3.) | texture_standardize
s = n | texture_normalize(sigmas=2.)
a = n | texture_normalize(sigmas=[0., 2.])
e = n | texture_equalize
q = texture_quantile(n, 0.7)
sd = texture_std(n)
"#,
    )
    .unwrap();
    let n = get_tex(&ctx, "n").as_interleaved();
    let count = n.len() as f32;
    let frac = |f: &dyn Fn(f32) -> bool| n.iter().filter(|&&v| f(v)).count() as f32 / count;

    let z = get_tex(&ctx, "z").as_interleaved();
    assert!(n.iter().zip(&z).all(|(a, b)| (a - b).abs() < 1e-4), "standardize(3n) must recover n");
    assert!((ctx.get_global("sd").unwrap().as_float().unwrap() - 1.).abs() < 1e-3);

    let s = get_tex(&ctx, "s").as_interleaved();
    assert!(s.iter().all(|&v| (0. ..=1.).contains(&v)));
    let clipped = s.iter().filter(|&&v| v == 0. || v == 1.).count() as f32 / count;
    assert!((0.02..0.08).contains(&clipped), "±2σ clips ~4.5%, got {clipped}");
    let a = get_tex(&ctx, "a").as_interleaved();
    let zeros = a.iter().filter(|&&v| v == 0.).count() as f32 / count;
    assert!((zeros - frac(&|v| v <= 0.)).abs() < 1e-3, "[0, 2] zeroes everything below the mean");

    let e = get_tex(&ctx, "e").as_interleaved();
    let e_mean = e.iter().sum::<f32>() / count;
    assert!((e_mean - 0.5).abs() < 0.01, "equalized mean {e_mean}");
    let above = e.iter().filter(|&&v| v > 0.7).count() as f32 / count;
    assert!((above - 0.3).abs() < 0.01, "equalize: {above} above 0.7");

    let q = ctx.get_global("q").unwrap().as_float().unwrap();
    assert!((frac(&|v| v > q) - 0.3).abs() < 0.002, "quantile(0.7) threshold covers 30%");

    let err = parse_and_eval_program("t = texture(4, 4, |uv| uv.x)\nt | texture_normalize(sigmas=[2., 1.])")
      .unwrap_err()
      .to_string();
    assert!(err.contains("lo < hi"), "{err}");
  }

  #[test]
  fn resize_basics() {
    let ctx = parse_and_eval_program(
      r#"
g = texture(8, 8, |uv| uv.x)
up = resize(16, 16, g)
down = resize(4, 4, g, filter="box")
nn = resize(4, 4, g, filter="nearest")
flat = texture(7, 3, |uv| 0.25) | resize(5, 9)
"#,
    )
    .unwrap();
    let up = get_tex(&ctx, "up");
    assert_eq!((up.width, up.height), (16, 16));
    let ud = up.as_interleaved();
    assert!(ud[0] < ud[7] && ud[7] < ud[15]);
    let down = get_tex(&ctx, "down");
    assert_eq!((down.width, down.height), (4, 4));
    // Box-downsampling a linear ramp: each output pixel is the mean of its 2px column pair.
    let dd = down.as_interleaved();
    assert!((dd[0] - (0.5 / 8. + 1.5 / 8.) / 2.).abs() < 1e-5, "{}", dd[0]);
    let nn = get_tex(&ctx, "nn");
    assert_eq!(nn.as_interleaved().len(), 16);
    for px in get_tex(&ctx, "flat").as_interleaved().iter() {
      assert!((px - 0.25).abs() < 1e-5, "{px}");
    }
  }

  #[test]
  fn resize_premultiplies_rgba() {
    // Transparent-green / opaque-blue seam: straight-alpha filtering would bleed green.
    let ctx = parse_and_eval_program(
      r#"
t = texture(16, 16, |uv| v4(0., 1. - floor(uv.x * 2.), floor(uv.x * 2.), floor(uv.x * 2.)))
d = resize(8, 8, t, filter="mitchell")
"#,
    )
    .unwrap();
    let d = get_tex(&ctx, "d");
    let px = &d.as_interleaved()[(4 * 8 + 6) * 4..(4 * 8 + 6) * 4 + 4];
    assert!(px[1] < 0.01, "green bled into opaque side: {px:?}");
    assert!(px[2] > 0.9, "blue should stay saturated: {px:?}");
  }

  #[test]
  fn dilate_erode_impulse_and_wrap() {
    let ctx = parse_and_eval_program(
      r#"
imp = texture(8, 8, |uv| 1. - min(1., floor(uv.x * 8.) + floor(uv.y * 8.)))
d = dilate(1, imp)
e = erode(1, d)
big = dilate(2, imp)
inv_e = erode(1, 1. - imp)
"#,
    )
    .unwrap();
    let d = get_tex(&ctx, "d");
    let dd = d.as_interleaved();
    // 3x3 box around (0,0), wrapping to the far edges.
    for y in 0..8i64 {
      for x in 0..8i64 {
        let inside = (x <= 1 || x == 7) && (y <= 1 || y == 7);
        assert_eq!(dd[(y * 8 + x) as usize], if inside { 1. } else { 0. }, "({x}, {y})");
      }
    }
    // Erosion after dilation (close) restores the single impulse plus nothing extra
    // is guaranteed only for shapes >= the SE; here just sanity-check counts.
    let ed = get_tex(&ctx, "e").as_interleaved();
    assert_eq!(ed.iter().filter(|&&v| v == 1.).count(), 1);
    assert_eq!(get_tex(&ctx, "big").as_interleaved().iter().filter(|&&v| v == 1.).count(), 25);
    let inv = get_tex(&ctx, "inv_e").as_interleaved();
    assert_eq!(inv.iter().filter(|&&v| v == 0.).count(), 9);
  }

  #[test]
  fn texture_levels_math_and_alpha() {
    let ctx = parse_and_eval_program(
      r#"
g = texture(4, 1, |uv| uv.x)
lv = texture_levels(0.25, 0.75, 0., 1., 1., g)
invl = g | texture_levels(0., 1., 1., 0., 1.)
gam = texture_levels(0., 1., 0., 1., 2., g)
rgba = texture(2, 1, |uv| v4(0.5, 0.5, 0.5, 0.25)) | texture_levels(0., 1., 0., 2., 1.)
"#,
    )
    .unwrap();
    let px = |name: &str| get_tex(&ctx, name).as_interleaved().to_vec();
    assert_eq!(px("lv"), [0., 0.25, 0.75, 1.]);
    assert_eq!(px("invl"), [0.875, 0.625, 0.375, 0.125]);
    let gam = px("gam");
    assert!((gam[0] - 0.125f32.sqrt()).abs() < 1e-6);
    let rgba = px("rgba");
    assert_eq!(&rgba[..4], &[1., 1., 1., 0.25], "alpha must be preserved");
  }

  #[test]
  fn input_image_levels_default_and_control() {
    let ctx = parse_and_eval_program(
      r#"
g = texture(4, 4, |uv| uv.x)
out = input_image_levels("lv", g)
outd = input_image_levels("lv2", g, default={in_hi: 0.5})
"#,
    )
    .unwrap();
    let g = get_tex(&ctx, "g").as_interleaved();
    let out = get_tex(&ctx, "out").as_interleaved();
    assert_eq!(&g[..], &out[..], "identity levels must round-trip pixels");
    let outd = get_tex(&ctx, "outd").as_interleaved();
    assert_eq!(outd[0], 0.25, "in_hi=0.5 doubles the black end");
    assert_eq!(outd[3], 1.);

    let controls = ctx.rendered_controls.inner.borrow();
    assert_eq!(controls.len(), 2);
    assert!(matches!(controls[0].kind, crate::ControlKind::ImageLevels));
    let stats = controls[0].stats.as_ref().unwrap();
    assert_eq!(stats.channels.len(), 1);
    assert_eq!((stats.channels[0].min, stats.channels[0].max), (g[0], g[3]));
    match &controls[1].current_value {
      Value::Map(m) => {
        assert_eq!(m.get("in_hi").unwrap().as_float().unwrap(), 0.5);
        assert_eq!(m.get("gamma").unwrap().as_float().unwrap(), 1.);
      }
      other => panic!("expected map control value, got {other:?}"),
    }
  }

  #[test]
  fn concat_channels_builds_rgba_and_accepts_consts() {
    let ctx = parse_and_eval_program(
      r#"
rgb = texture(4, 2, |uv| v3(uv.x, uv.y, 0.25))
m = texture(4, 2, |uv| uv.x * 0.5)
rgba = concat_channels(rgb, m)
piped = m | concat_channels(rgb)
gray_a = concat_channels(m, 1)
opaque = concat_channels(1., 0., 0.5, m)
"#,
    )
    .unwrap();
    let rgb = get_tex(&ctx, "rgb").as_interleaved();
    let m = get_tex(&ctx, "m").as_interleaved();
    let rgba = get_tex(&ctx, "rgba");
    assert_eq!(rgba.channels, 4);
    let rd = rgba.as_interleaved();
    for i in 0..8 {
      assert_eq!(&rd[i * 4..i * 4 + 3], &rgb[i * 3..i * 3 + 3], "rgb at {i}");
      assert_eq!(rd[i * 4 + 3], m[i], "alpha at {i}");
    }
    assert_eq!(get_tex(&ctx, "piped").as_interleaved(), rd, "partial application fills the last slot");
    let ga = get_tex(&ctx, "gray_a");
    assert_eq!(ga.channels, 2);
    assert_eq!(&ga.as_interleaved()[..4], &[m[0], 1., m[1], 1.]);
    let op = get_tex(&ctx, "opaque");
    assert_eq!((op.channels, op.width, op.height), (4, 4, 2));
    assert_eq!(&op.as_interleaved()[..4], &[1., 0., 0.5, m[0]]);
  }

  #[test]
  fn concat_channels_views_match_materialized() {
    let ctx = parse_and_eval_program(
      r#"
t = texture(4, 4, |uv| v3(uv.x, uv.y, 0.5))
v = concat_channels(t.bgr, flip_x(t).r)
d = concat_channels(materialize(t.bgr), materialize(flip_x(t).r))
"#,
    )
    .unwrap();
    assert_eq!(get_tex(&ctx, "v").as_interleaved(), get_tex(&ctx, "d").as_interleaved());
  }

  #[test]
  fn concat_channels_errors() {
    let cases = [
      "a = texture(4, 4, |uv| uv.x)\nb = texture(4, 2, |uv| uv.x)\nc = concat_channels(a, b)",
      "a = texture(4, 4, |uv| v4(0., 0., 0., 1.))\nc = concat_channels(a, 1.)",
      "c = concat_channels(0.5, 1.)",
    ];
    for src in cases {
      assert!(parse_and_eval_program(src).is_err(), "expected error for:\n{src}");
    }
  }

  #[test]
  fn concat_channels_alpha_mask_blits_over_rgb_base() {
    let ctx = parse_and_eval_program(
      r#"
base = texture(4, 4, |uv| v3(0., 0., 0.))
dirt = texture(4, 4, |uv| v3(1., 1., 1.))
height = texture(4, 4, |uv| 0.25)
masked = concat_channels(dirt, height * 2.)
out = blit(masked, base, filter="nearest")
"#,
    )
    .unwrap();
    assert_eq!(get_tex(&ctx, "masked").channels, 4);
    let out = get_tex(&ctx, "out");
    assert_eq!(out.channels, 3);
    for px in out.as_interleaved().iter() {
      assert!((px - 0.5).abs() < 1e-6, "alpha 0.5 over black base: got {px}");
    }
  }

  #[test]
  fn empty_swizzle_errors_instead_of_making_a_zero_channel_texture() {
    // A 0-channel handle panics `chunks_exact(0)` in every dense op, which aborts the
    // whole eval under wasm rather than surfacing an error.
    let err = parse_and_eval_program("t = texture(4, 4, |uv| uv.x)\nq = materialize(t[\"\"])")
      .unwrap_err()
      .to_string();
    assert!(err.contains("1 to 4 chars"), "{err}");
  }

  #[test]
  fn image_levels_default_rejects_typos() {
    let bad = parse_and_eval_program(
      "g = texture(4, 4, |uv| uv.x)\nout = input_image_levels(\"lv\", g, default={in_high: 0.5})",
    )
    .unwrap_err()
    .to_string();
    assert!(bad.contains("unknown `default` key"), "{bad}");
    assert!(parse_and_eval_program(
      "g = texture(4, 4, |uv| uv.x)\nout = input_image_levels(\"lv\", g, default={in_hi: 0.5})"
    )
    .is_ok());
  }

  #[test]
  fn dilate_zero_radius_is_identity() {
    let ctx = parse_and_eval_program(
      "t = texture(4, 4, |uv| uv.x)\nd = dilate(0, t)",
    )
    .unwrap();
    assert_eq!(get_tex(&ctx, "t").storage_id(), get_tex(&ctx, "d").storage_id());
  }
}
