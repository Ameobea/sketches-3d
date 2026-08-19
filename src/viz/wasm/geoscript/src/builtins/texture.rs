use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  ArgRef, Callable, ErrorStack, EvalCtx, Mat4, RenderedTexture, Sym, TexStorage, TextureHandle,
  TextureUsage, TextureWrap, Value, Vec2, Vec3, Vec4, EMPTY_KWARGS,
};

pub(crate) const MAX_TEXTURE_DIM: i64 = 8192;

impl TextureHandle {
  pub(crate) fn wrap_coord(&self, c: i64, n: usize) -> usize {
    let n = n as i64;
    (match self.wrap {
      TextureWrap::Repeat => c.rem_euclid(n),
      TextureWrap::Clamp => c.clamp(0, n - 1),
      TextureWrap::Mirror => {
        let m = c.rem_euclid(2 * n);
        if m < n {
          m
        } else {
          2 * n - 1 - m
        }
      }
    }) as usize
  }

  pub(crate) fn texel(&self, x: i64, y: i64, chan: usize) -> f32 {
    let x = self.wrap_coord(x, self.width);
    let y = self.wrap_coord(y, self.height);
    self.texel_raw(x, y, chan)
  }
}

pub(crate) fn texture_zip(
  a: &TextureHandle,
  b: &TextureHandle,
  op: &str,
  f: impl Fn(f32, f32) -> f32,
) -> Result<Value, ErrorStack> {
  if (a.width, a.height, a.channels) != (b.width, b.height, b.channels) {
    return Err(ErrorStack::new(format!(
      "texture {op} texture requires matching dims and channels; found {}x{}x{}ch vs {}x{}x{}ch",
      a.width, a.height, a.channels, b.width, b.height, b.channels
    )));
  }
  let pixels = match (a.dense_pixels(), b.dense_pixels()) {
    (Some(pa), Some(pb)) => pa.iter().zip(pb.iter()).map(|(&x, &y)| f(x, y)).collect(),
    _ => {
      let mut out = Vec::with_capacity(a.width * a.height * a.channels);
      for y in 0..a.height {
        for x in 0..a.width {
          for c in 0..a.channels {
            out.push(f(a.texel_raw(x, y, c), b.texel_raw(x, y, c)));
          }
        }
      }
      out
    }
  };
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::Dense(Rc::new(pixels)),
    mips: Default::default(),
    ..a.clone()
  })))
}

/// Elementwise map with the channel index available; the dense/view split every unary
/// texture op needs.
pub(crate) fn texture_map_chan(t: &TextureHandle, f: impl Fn(f32, usize) -> f32) -> Value {
  let ch = t.channels;
  let pixels: Vec<f32> = match t.dense_pixels() {
    // Chunked rather than `i % ch`: `ch` is a runtime value, so that's an integer division
    // per texel — several times the cost of the op it decorates.
    Some(px) => {
      let mut out = Vec::with_capacity(px.len());
      for texel in px.chunks_exact(ch) {
        for (c, &x) in texel.iter().enumerate() {
          out.push(f(x, c));
        }
      }
      out
    }
    None => {
      let mut out = Vec::with_capacity(t.width * t.height * ch);
      for y in 0..t.height {
        for x in 0..t.width {
          for c in 0..ch {
            out.push(f(t.texel_raw(x, y, c), c));
          }
        }
      }
      out
    }
  };
  Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::Dense(Rc::new(pixels)),
    mips: Default::default(),
    ..t.clone()
  }))
}

pub(crate) fn texture_map_unary(t: &TextureHandle, f: impl Fn(f32) -> f32) -> Value {
  texture_map_chan(t, |x, _| f(x))
}

pub(crate) fn texture_scale(t: &TextureHandle, s: f32) -> Value {
  texture_map_unary(t, |x| x * s)
}

/// Per-channel-constant broadcast: `f(texel, v[c])` per channel. Strict: vec len must
/// equal the texture's channel count.
pub(crate) fn texture_zip_vec(
  t: &TextureHandle,
  v: &[f32],
  op: &str,
  f: impl Fn(f32, f32) -> f32,
) -> Result<Value, ErrorStack> {
  if t.channels != v.len() {
    return Err(ErrorStack::new(format!(
      "texture {op} vec{} requires a {}-channel texture; found {} channel(s)",
      v.len(),
      v.len(),
      t.channels
    )));
  }
  Ok(texture_map_chan(t, |x, c| f(x, v[c])))
}

fn channels_value(vals: [f32; 4], channels: usize) -> Value {
  match channels {
    1 => Value::Float(vals[0]),
    2 => Value::Vec2(Vec2::new(vals[0], vals[1])),
    3 => Value::Vec3(Vec3::new(vals[0], vals[1], vals[2])),
    _ => Value::Vec4(Rc::new(Vec4::new(vals[0], vals[1], vals[2], vals[3]))),
  }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ReduceKind {
  Min,
  Max,
  Mean,
}

/// Per-channel single-pass reduction.
pub(crate) fn texture_reduce(kind: ReduceKind, t: &TextureHandle) -> [f32; 4] {
  let ch = t.channels;
  let mut acc = [match kind {
    ReduceKind::Min => f32::INFINITY,
    ReduceKind::Max => f32::NEG_INFINITY,
    ReduceKind::Mean => 0.,
  }; 4];
  let mut mean_acc = [0f64; 4];
  let mut fold = |c: usize, x: f32| match kind {
    ReduceKind::Min => acc[c] = acc[c].min(x),
    ReduceKind::Max => acc[c] = acc[c].max(x),
    ReduceKind::Mean => mean_acc[c] += x as f64,
  };
  match t.dense_pixels() {
    Some(px) => {
      for texel in px.chunks_exact(ch) {
        for (c, &x) in texel.iter().enumerate() {
          fold(c, x);
        }
      }
    }
    None => {
      for y in 0..t.height {
        for x in 0..t.width {
          for c in 0..ch {
            fold(c, t.texel_raw(x, y, c));
          }
        }
      }
    }
  }
  if kind == ReduceKind::Mean {
    let n = (t.width * t.height) as f64;
    for c in 0..ch {
      acc[c] = (mean_acc[c] / n) as f32;
    }
  }
  acc
}

pub(crate) fn texture_reduce_impl(
  kind: ReduceKind,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let t = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  Ok(channels_value(texture_reduce(kind, t), t.channels))
}

enum TexIx {
  Int(i64),
  /// Half-open; `None` end = to the end of the axis.
  Range(i64, Option<i64>),
}

fn parse_tex_ix(v: &Value) -> Result<TexIx, ErrorStack> {
  match v {
    Value::Int(i) => Ok(TexIx::Int(*i)),
    Value::Sequence(seq) => {
      let any: &dyn std::any::Any = &**seq;
      match any.downcast_ref::<crate::seq::IntRange>() {
        Some(r) => Ok(TexIx::Range(r.start, r.end)),
        None => Err(ErrorStack::new(
          "indexing a texture with an arbitrary sequence (gather / fancy indexing) is not \
           supported; use an int or a range like `a..b`",
        )),
      }
    }
    other => Err(ErrorStack::new(format!(
      "texture indices must be ints or ranges, found: {other:?}"
    ))),
  }
}

fn resolve_tex_int(i: i64, len: usize, axis: &str) -> Result<usize, ErrorStack> {
  if i < 0 {
    return Err(ErrorStack::new(format!(
      "negative {axis} index {i} not supported for textures"
    )));
  }
  if i as usize >= len {
    return Err(ErrorStack::new(format!(
      "{axis} index {i} out of bounds; len={len}"
    )));
  }
  Ok(i as usize)
}

/// -> (start, len)
pub(crate) fn resolve_tex_range(
  start: i64,
  end: Option<i64>,
  len: usize,
  axis: &str,
) -> Result<(usize, usize), ErrorStack> {
  if start < 0 {
    return Err(ErrorStack::new(format!(
      "negative {axis} index {start} not supported for textures"
    )));
  }
  let end = end.unwrap_or(len as i64);
  if end <= start {
    return Err(ErrorStack::new(format!(
      "empty {axis} range {start}..{end} for texture indexing"
    )));
  }
  if end > len as i64 {
    return Err(ErrorStack::new(format!(
      "{axis} range {start}..{end} out of bounds; len={len}"
    )));
  }
  Ok((start as usize, (end - start) as usize))
}

fn pixel_value(t: &TextureHandle, x: usize, y: usize) -> Value {
  let mut v = [0f32; 4];
  for (c, out) in v[..t.channels].iter_mut().enumerate() {
    *out = t.texel_raw(x, y, c);
  }
  channels_value(v, t.channels)
}

fn tex_val(t: TextureHandle) -> Value {
  Value::Texture(Rc::new(t))
}

/// `t[ix0]` / `t[ix0, ix1]`. Axis order is row-major, channel-last; the single-bracket
/// form indexes the outermost non-degenerate axis (rows when height > 1, else cols, where
/// an int yields the pixel value — so `lut[i]` on a 1-tall LUT gives the pixel directly).
pub(crate) fn texture_index(
  t: &TextureHandle,
  ix0: &Value,
  ix1: Option<&Value>,
) -> Result<Value, ErrorStack> {
  let ix0 = parse_tex_ix(ix0)?;
  let Some(ix1) = ix1 else {
    return Ok(if t.height > 1 {
      match ix0 {
        TexIx::Int(i) => {
          let y = resolve_tex_int(i, t.height, "row")?;
          tex_val(t.crop_view(0, y, t.width, 1))
        }
        TexIx::Range(s, e) => {
          let (y0, h) = resolve_tex_range(s, e, t.height, "row")?;
          tex_val(t.crop_view(0, y0, t.width, h))
        }
      }
    } else {
      match ix0 {
        TexIx::Int(i) => pixel_value(t, resolve_tex_int(i, t.width, "col")?, 0),
        TexIx::Range(s, e) => {
          let (x0, w) = resolve_tex_range(s, e, t.width, "col")?;
          tex_val(t.crop_view(x0, 0, w, 1))
        }
      }
    });
  };
  let ix1 = parse_tex_ix(ix1)?;
  Ok(match (ix0, ix1) {
    (TexIx::Int(r), TexIx::Int(c)) => {
      let y = resolve_tex_int(r, t.height, "row")?;
      let x = resolve_tex_int(c, t.width, "col")?;
      pixel_value(t, x, y)
    }
    (TexIx::Int(r), TexIx::Range(s, e)) => {
      let y = resolve_tex_int(r, t.height, "row")?;
      let (x0, w) = resolve_tex_range(s, e, t.width, "col")?;
      tex_val(t.crop_view(x0, y, w, 1))
    }
    (TexIx::Range(s, e), TexIx::Int(c)) => {
      let (y0, h) = resolve_tex_range(s, e, t.height, "row")?;
      let x = resolve_tex_int(c, t.width, "col")?;
      tex_val(t.crop_view(x, y0, 1, h))
    }
    (TexIx::Range(rs, re), TexIx::Range(cs, ce)) => {
      let (y0, h) = resolve_tex_range(rs, re, t.height, "row")?;
      let (x0, w) = resolve_tex_range(cs, ce, t.width, "col")?;
      tex_val(t.crop_view(x0, y0, w, h))
    }
  })
}

fn pixel_from_value(v: &Value) -> Option<([f32; 4], usize)> {
  match v {
    Value::Vec2(v) => Some(([v.x, v.y, 0., 0.], 2)),
    Value::Vec3(v) => Some(([v.x, v.y, v.z, 0.], 3)),
    Value::Vec4(v) => Some(([v.x, v.y, v.z, v.w], 4)),
    v => v.as_float().map(|f| ([f, 0., 0., 0.], 1)),
  }
}

pub(crate) fn texture_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let width = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
  let height = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
  let generator = arg_refs[2].resolve(args, kwargs).as_callable().unwrap();
  let wrap = TextureWrap::from_name(arg_refs[3].resolve(args, kwargs).as_str().unwrap())?;

  if width < 1 || height < 1 || width > MAX_TEXTURE_DIM || height > MAX_TEXTURE_DIM {
    return Err(ErrorStack::new(format!(
      "Invalid texture dims {width}x{height}; expected 1..={MAX_TEXTURE_DIM} per side"
    )));
  }
  let (w, h) = (width as usize, height as usize);

  let mut pixels: Vec<f32> = Vec::new();
  let mut channels = 0usize;
  for y in 0..h {
    for x in 0..w {
      let uv = Vec2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
      let out = ctx
        .invoke_callable(
          generator,
          &[
            Value::Vec2(uv),
            Value::Int(x as i64),
            Value::Int(y as i64),
          ],
          EMPTY_KWARGS,
        )
        .map_err(|err| {
          err.wrap("Error produced by user-supplied `generator` callable in `texture`")
        })?;
      let (px, n) = pixel_from_value(&out).ok_or_else(|| {
        ErrorStack::new(format!(
          "Expected float, vec2, vec3, or vec4 from `generator` callable in `texture`, found: \
           {out:?}"
        ))
      })?;
      if channels == 0 {
        channels = n;
        pixels.reserve_exact(w * h * n);
      } else if n != channels {
        return Err(ErrorStack::new(
          "`generator` callable in `texture` returned a mix of float/vec2/vec3/vec4 values",
        ));
      }
      pixels.extend_from_slice(&px[..n]);
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::Dense(Rc::new(pixels)),
    width: w,
    height: h,
    channels,
    wrap,
    min_filter: None,
    mag_filter: None,
    format: None,
    transform: Mat4::identity(),
    mips: Default::default(),
  })))
}

pub(crate) fn map_texture_impl(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  tex: &TextureHandle,
) -> Result<Value, ErrorStack> {
  let (w, h, ch) = (tex.width, tex.height, tex.channels);
  let src_rc = tex.as_dense();
  let src: &[f32] = &src_rc;
  let mut out: Vec<f32> = Vec::new();
  let mut out_ch = 0usize;
  for y in 0..h {
    for x in 0..w {
      let base = (y * w + x) * ch;
      let mut v = [0f32; 4];
      v[..ch].copy_from_slice(&src[base..base + ch]);
      let val = channels_value(v, ch);
      let uv = Vec2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
      let res = ctx
        .invoke_callable(
          cb,
          &[
            val,
            Value::Vec2(uv),
            Value::Int(x as i64),
            Value::Int(y as i64),
          ],
          EMPTY_KWARGS,
        )
        .map_err(|err| err.wrap("Error produced by callable passed to `map` over texture"))?;
      let (px, n) = pixel_from_value(&res).ok_or_else(|| {
        ErrorStack::new(format!(
          "Expected float, vec2, vec3, or vec4 from callable passed to `map` over texture, found: \
           {res:?}"
        ))
      })?;
      if out_ch == 0 {
        out_ch = n;
        out.reserve_exact(w * h * n);
      } else if n != out_ch {
        return Err(ErrorStack::new(
          "callable passed to `map` over texture returned a mix of float/vec2/vec3/vec4 values",
        ));
      }
      out.extend_from_slice(&px[..n]);
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    channels: out_ch,
    storage: TexStorage::Dense(Rc::new(out)),
    mips: Default::default(),
    ..tex.clone()
  })))
}

pub(crate) fn blur_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let sigma = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
  let tex_val = arg_refs[1].resolve(args, kwargs);
  let tex = tex_val.as_texture().unwrap();
  if sigma <= 0. {
    return Ok(tex_val.clone());
  }
  Ok(Value::Texture(Rc::new(blur_tex(sigma, tex))))
}

/// Materialize preserving handle identity: a dense handle is shared, not re-wrapped, so
/// const-fold cache hits keep replaying the same `Rc`.
pub(crate) fn dense_rc(t: &Rc<TextureHandle>) -> Rc<TextureHandle> {
  if t.is_dense() {
    Rc::clone(t)
  } else {
    Rc::new(t.dense_clone())
  }
}

/// Dense copy with RGBA premultiplied so transparent texels' RGB doesn't bleed into
/// visible ones under filtering; a no-op copy below 4 channels.
pub(crate) fn premultiplied_dense(tex: &TextureHandle) -> TextureHandle {
  if tex.channels != 4 {
    return tex.dense_clone();
  }
  let mut px = tex.as_dense().to_vec();
  for p in px.chunks_exact_mut(4) {
    let a = p[3];
    p[0] *= a;
    p[1] *= a;
    p[2] *= a;
  }
  TextureHandle {
    storage: TexStorage::Dense(Rc::new(px)),
    mips: Default::default(),
    ..tex.clone()
  }
}

pub(crate) fn unpremultiply(px: &mut [f32]) {
  for p in px.chunks_exact_mut(4) {
    let a = p[3];
    if a > 1e-8 {
      p[0] /= a;
      p[1] /= a;
      p[2] /= a;
    }
  }
}

/// Separable gaussian; caller guarantees `sigma > 0`.
pub(crate) fn blur_tex(sigma: f32, tex: &TextureHandle) -> TextureHandle {
  let (w, h, ch) = (tex.width, tex.height, tex.channels);
  let half = ((sigma * 3.).ceil() as i64).max(1);
  let mut weights = Vec::with_capacity(half as usize + 1);
  for i in 0..=half {
    weights.push((-((i * i) as f32) / (2. * sigma * sigma)).exp());
  }
  let norm = weights[0] + 2. * weights[1..].iter().sum::<f32>();
  for wt in &mut weights {
    *wt /= norm;
  }

  let pass = |src: &TextureHandle, dx: i64, dy: i64| -> Vec<f32> {
    let px: &[f32] = src.dense_pixels().unwrap();
    let stride = (if dx == 1 { 1 } else { w }) * ch;
    let lim = (if dx == 1 { w } else { h }) as i64;
    let mut out = vec![0f32; w * h * ch];
    for y in 0..h {
      for x in 0..w {
        let base = (y * w + x) * ch;
        let pos = (if dx == 1 { x } else { y }) as i64;
        if pos >= half && pos + half < lim {
          // Interior: no tap can wrap, so index directly instead of paying
          // `wrap_coord`'s rem_euclid + branch per tap.
          for c in 0..ch {
            let mut acc = weights[0] * px[base + c];
            for i in 1..=half as usize {
              acc += weights[i] * (px[base + c - i * stride] + px[base + c + i * stride]);
            }
            out[base + c] = acc;
          }
        } else {
          for c in 0..ch {
            let mut acc = weights[0] * src.texel(x as i64, y as i64, c);
            for i in 1..=half {
              acc += weights[i as usize]
                * (src.texel(x as i64 - i * dx, y as i64 - i * dy, c)
                  + src.texel(x as i64 + i * dx, y as i64 + i * dy, c));
            }
            out[base + c] = acc;
          }
        }
      }
    }
    out
  };

  let src = premultiplied_dense(tex);

  let mid = TextureHandle {
    storage: TexStorage::Dense(Rc::new(pass(&src, 1, 0))),
    mips: Default::default(),
    ..src
  };
  let mut out = pass(&mid, 0, 1);
  if ch == 4 {
    unpremultiply(&mut out);
  }
  TextureHandle {
    storage: TexStorage::Dense(Rc::new(out)),
    mips: Default::default(),
    ..mid
  }
}

pub(crate) fn height_to_normal_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (strength, tex) = match def_ix {
    0 => (1., arg_refs[0].resolve(args, kwargs).as_texture().unwrap()),
    1 => (
      arg_refs[0].resolve(args, kwargs).as_float().unwrap(),
      arg_refs[1].resolve(args, kwargs).as_texture().unwrap(),
    ),
    _ => unimplemented!(),
  };

  let tex = tex.dense_clone();
  let (w, h) = (tex.width, tex.height);
  let mut out = Vec::with_capacity(w * h * 3);
  for y in 0..h {
    for x in 0..w {
      let (x, y) = (x as i64, y as i64);
      let dx = (tex.texel(x + 1, y, 0) - tex.texel(x - 1, y, 0)) * 0.5 * strength;
      let dy = (tex.texel(x, y + 1, 0) - tex.texel(x, y - 1, 0)) * 0.5 * strength;
      let inv_len = 1. / (dx * dx + dy * dy + 1.).sqrt();
      out.extend_from_slice(&[
        -dx * inv_len * 0.5 + 0.5,
        -dy * inv_len * 0.5 + 0.5,
        inv_len * 0.5 + 0.5,
      ]);
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    channels: 3,
    storage: TexStorage::Dense(Rc::new(out)),
    ..tex
  })))
}

pub(crate) fn render_texture_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let tex = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let name = arg_refs[1].resolve(args, kwargs).as_str().unwrap();
  let usage = match arg_refs[2].resolve(args, kwargs) {
    Value::Nil => None,
    v => Some(TextureUsage::from_name(v.as_str().unwrap())?),
  };
  // Materialize at the render boundary so the host getters, which each read the pixels
  // independently, don't re-interleave a view once per read.
  ctx.rendered_textures.push(RenderedTexture {
    texture: dense_rc(tex),
    extra_slices: Vec::new(),
    name: name.to_owned(),
    usage,
    source_module: ctx.current_module.borrow().clone(),
    texture_id: ctx.next_render_id(),
  });
  Ok(Value::Nil)
}

const MAX_STACK_LAYERS: usize = 256;

pub(crate) fn render_texture_stack_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
  let name = arg_refs[1].resolve(args, kwargs).as_str().unwrap();
  let usage = match arg_refs[2].resolve(args, kwargs) {
    Value::Nil => None,
    v => Some(TextureUsage::from_name(v.as_str().unwrap())?),
  };

  let mut slices: Vec<Rc<TextureHandle>> = Vec::new();
  for (i, val) in seq.consume(ctx).enumerate() {
    let val = val.map_err(|err| err.wrap("Error produced by `slices` seq in `render_texture_stack`"))?;
    let Value::Texture(tex) = val else {
      return Err(ErrorStack::new(format!(
        "Expected texture at index {i} of `slices` in `render_texture_stack`, found: {val:?}"
      )));
    };
    if let Some(first) = slices.first() {
      if (tex.width, tex.height, tex.channels, tex.wrap)
        != (first.width, first.height, first.channels, first.wrap)
      {
        return Err(ErrorStack::new(format!(
          "All slices in `render_texture_stack` must have matching dims/channels/wrap; slice 0 is \
           {}x{}x{}ch wrap={:?} but slice {i} is {}x{}x{}ch wrap={:?}",
          first.width, first.height, first.channels, first.wrap, tex.width, tex.height, tex.channels,
          tex.wrap
        )));
      }
    }
    if slices.len() >= MAX_STACK_LAYERS {
      return Err(ErrorStack::new(format!(
        "`render_texture_stack` supports at most {MAX_STACK_LAYERS} slices"
      )));
    }
    slices.push(tex);
  }
  if slices.len() < 2 {
    return Err(ErrorStack::new(format!(
      "`render_texture_stack` requires at least 2 slices, found {}",
      slices.len()
    )));
  }

  let mut slices = slices.iter().map(dense_rc);
  ctx.rendered_textures.push(RenderedTexture {
    texture: slices.next().unwrap(),
    extra_slices: slices.collect(),
    name: name.to_owned(),
    usage,
    source_module: ctx.current_module.borrow().clone(),
    texture_id: ctx.next_render_id(),
  });
  Ok(Value::Nil)
}

#[cfg(test)]
mod tests {
  use crate::{
    eval_program_with_ctx, optimizer::optimize_ast, parse_and_eval_program, parse_program_src,
    EvalCtx,
  };

  #[test]
  fn texture_generator_and_render() {
    let ctx = parse_and_eval_program(
      r#"texture(4, 2, |uv| uv.x) | render_texture(name="height", usage="height")"#,
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 1);
    let t = &rendered[0];
    assert_eq!(t.name, "height");
    assert_eq!(t.usage, Some(crate::TextureUsage::Height));

    let err = parse_and_eval_program(r#"texture(1, 1, |uv| 0.) | render_texture(usage="bogus")"#)
      .unwrap_err();
    assert!(err.to_string().contains("Invalid texture usage"));
    let tex = &t.texture;
    assert_eq!((tex.width, tex.height, tex.channels), (4, 2, 1));
    assert_eq!(tex.as_dense().len(), 8);
    for y in 0..2 {
      for x in 0..4 {
        assert_eq!(tex.as_dense()[y * 4 + x], (x as f32 + 0.5) / 4.);
      }
    }
  }

  #[test]
  fn texture_generator_pixel_indices() {
    let ctx = parse_and_eval_program(
      r#"
ix = texture(4, 2, |uv, x, y| v2(float(x), float(y)))
uv_only = texture(4, 2, |uv| uv.x)
"#,
    )
    .unwrap();
    let get = |name: &str| match ctx.get_global(name).unwrap() {
      crate::Value::Texture(t) => t,
      other => panic!("expected texture, got {other:?}"),
    };
    let ix = get("ix").as_dense().to_vec();
    for y in 0..2 {
      for x in 0..4 {
        let base = (y * 4 + x) * 2;
        assert_eq!((ix[base], ix[base + 1]), (x as f32, y as f32));
      }
    }
    // A closure declaring only `uv` still works; the extra args are dropped.
    assert_eq!(get("uv_only").as_dense()[0], 0.125);
  }

  #[test]
  fn render_texture_stack_basic_and_validation() {
    let ctx = parse_and_eval_program(
      r#"
s0 = texture(4, 2, |uv| uv.x)
s1 = texture(4, 2, |uv| uv.y)
s2 = texture(4, 2, |uv| 1.)
[s0, s1, s2] | render_texture_stack(name="wall", usage="albedo")
"#,
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 1);
    let t = &rendered[0];
    assert_eq!(t.name, "wall");
    assert_eq!(t.usage, Some(crate::TextureUsage::Albedo));
    assert_eq!(t.extra_slices.len(), 2);
    assert_eq!((t.texture.width, t.texture.height, t.texture.channels), (4, 2, 1));
    assert_eq!(t.texture.as_dense()[0], 0.5 / 4.);
    assert_eq!(t.extra_slices[1].as_dense()[0], 1.);

    let err = parse_and_eval_program(r#"[texture(2, 2, |uv| 0.)] | render_texture_stack(name="x")"#)
      .unwrap_err();
    assert!(err.to_string().contains("at least 2 slices"));

    let err = parse_and_eval_program(
      r#"[texture(2, 2, |uv| 0.), texture(4, 2, |uv| 0.)] | render_texture_stack(name="x")"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("matching dims"));

    let err =
      parse_and_eval_program(r#"[texture(2, 2, |uv| 0.), 3] | render_texture_stack(name="x")"#)
        .unwrap_err();
    assert!(err.to_string().contains("Expected texture at index 1"));
  }

  #[test]
  fn render_texture_stack_survives_rerun() {
    let ctx = EvalCtx::default();
    let src = r#"[texture(2, 2, |uv| 0.), texture(2, 2, |uv| 1.)] | render_texture_stack(name="s")"#;
    let mut ast = parse_program_src(&ctx, src).unwrap();
    for run in 1..=2 {
      ctx.rendered_textures.inner.borrow_mut().clear();
      optimize_ast(&ctx, &mut ast).unwrap();
      eval_program_with_ctx(&ctx, &ast).unwrap();
      assert_eq!(ctx.rendered_textures.len(), 1, "run {run}");
    }
  }

  #[test]
  fn render_texture_stack_module_cache_replay_and_param_baking() {
    let ctx = EvalCtx::default();
    ctx.module_sources.borrow_mut().insert(
      "textab:root".to_string(),
      r#"
[texture(2, 2, |uv| 0.), texture(2, 2, |uv| uv.x), texture(2, 2, |uv| 1.)]
  | render_texture_stack(name="stack", usage="mask")
export marker = 1
"#
      .to_string(),
    );
    ctx.injected_texture_params.borrow_mut().insert(
      "textab\0stack".to_string(),
      crate::InjectedTextureParams {
        format: Some(crate::TextureFormat::R32F),
        ..Default::default()
      },
    );
    let src = r#"import { marker } from "textab:root""#;
    for run in 1..=2 {
      ctx.rendered_textures.inner.borrow_mut().clear();
      ctx.replayed_this_run.borrow_mut().clear();
      crate::parse_and_eval_program_with_ctx(src.to_string(), &ctx, false).unwrap();
      ctx.apply_injected_texture_params();
      let rendered = ctx.rendered_textures.inner.borrow();
      assert_eq!(rendered.len(), 1, "run {run}");
      assert_eq!(rendered[0].extra_slices.len(), 2, "run {run}");
      assert_eq!(rendered[0].source_module.as_deref(), Some("textab:root"), "run {run}");
      assert_eq!(rendered[0].texture.format, Some(crate::TextureFormat::R32F), "run {run}");
    }
    assert!(ctx.module_exports.borrow().contains_key("textab:root"));
  }

  #[test]
  fn texture_pixel_map() {
    let ctx = parse_and_eval_program(
      r#"
t = texture(4, 2, |uv| uv.x)
(t -> |val, uv, x_ix, y_ix| v3(val, uv.y, x_ix + y_ix)) | render_texture(name="rgb")
(t -> |val| val * 2.) | render_texture(name="doubled")
"#,
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 2);

    let rgb = &rendered[0].texture;
    assert_eq!((rgb.width, rgb.height, rgb.channels), (4, 2, 3));
    for y in 0..2 {
      for x in 0..4 {
        let base = (y * 4 + x) * 3;
        assert_eq!(rgb.as_dense()[base], (x as f32 + 0.5) / 4.);
        assert_eq!(rgb.as_dense()[base + 1], (y as f32 + 0.5) / 2.);
        assert_eq!(rgb.as_dense()[base + 2], (x + y) as f32);
      }
    }

    let doubled = &rendered[1].texture;
    assert_eq!(doubled.channels, 1);
    assert_eq!(doubled.as_dense()[1], 2. * (1.5 / 4.));
  }

  #[test]
  fn blur_wrap_modes_and_flat_normals() {
    // Impulse at pixel (0,0): 1 - min(1, floor(u*8) + floor(v*8))
    let src = r#"
gen = |uv| 1. - min(1., floor(uv.x * 8.) + floor(uv.y * 8.))
texture(8, 8, gen) | blur(1.) | render_texture(name="wrapped")
texture(8, 8, gen, wrap="clamp") | blur(1.) | render_texture(name="clamped")
texture(4, 4, |uv| 0.5) | height_to_normal | render_texture(name="flat_n")
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 3);

    // Repeat wrap leaks the impulse across the seam to (7,0); clamp doesn't.
    let wrapped = &rendered[0].texture;
    let clamped = &rendered[1].texture;
    assert!(wrapped.as_dense()[7] > 0.);
    assert_eq!(clamped.as_dense()[7], 0.);

    let flat_n = &rendered[2].texture;
    assert_eq!(flat_n.channels, 3);
    for px in flat_n.as_dense().chunks(3) {
      assert_eq!(px, &[0.5, 0.5, 1.0]);
    }
  }

  #[test]
  fn texture_arithmetic_and_ramp_apply() {
    let ctx = parse_and_eval_program(
      r#"
a = texture(2, 2, |uv| uv.x)
b = texture(2, 2, |uv| 1.)
(a + b) | render_texture(name="sum")
(2. * a * 0.5) | render_texture(name="scaled")
(a * b) | render_texture(name="prod")
r = color_ramp(stops=[srgb(0x000000), srgb(0xffffff)])
r(a) | render_texture(name="colored")
"#,
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 4);
    let px = |i: usize| rendered[i].texture.as_dense();
    assert_eq!(px(0)[0], 0.25 + 1.);
    assert_eq!(px(1)[0], 0.25);
    assert_eq!(px(1)[1], 0.75);
    assert_eq!(px(2)[1], 0.75);
    let colored = &rendered[3].texture;
    assert_eq!(colored.channels, 3);
    assert!(colored.as_dense()[0] < colored.as_dense()[3]);

    let err = parse_and_eval_program(r#"texture(2, 2, |uv| 0.) + texture(4, 2, |uv| 0.)"#)
      .unwrap_err();
    assert!(err.to_string().contains("matching dims"), "{err}");

    let err = parse_and_eval_program(
      r#"
r = color_ramp(stops=[srgb(0x000000), srgb(0xffffff)])
r(texture(2, 2, |uv| v3(uv.x, 0., 0.)))
"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("1 channel"), "{err}");
  }

  #[test]
  fn texture_vec4_roundtrip() {
    let ctx = parse_and_eval_program(
      r#"
t = texture(2, 2, |uv| v4(uv.x, uv.y, 1., 0.5))
(t -> |val| val.a) | render_texture(name="alpha")
t | render_texture(name="rgba")
"#,
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    assert_eq!(rendered.len(), 2);

    let alpha = &rendered[0].texture;
    assert_eq!(alpha.channels, 1);
    assert!(alpha.as_dense().iter().all(|&p| p == 0.5));

    let rgba = &rendered[1].texture;
    assert_eq!((rgba.width, rgba.height, rgba.channels), (2, 2, 4));
    assert_eq!(rgba.as_dense()[0..4], [0.25, 0.25, 1., 0.5]);
  }

  /// `render_texture` pushes to `rendered_textures` as a side effect, so it must not be
  /// const-folded (same regression class as `path_render`).
  #[test]
  fn render_texture_survives_rerun() {
    let ctx = EvalCtx::default();
    let src = "texture(2, 2, |uv| uv.x) | render_texture";
    let mut ast = parse_program_src(&ctx, src).unwrap();

    for run in 1..=2 {
      ctx.rendered_textures.inner.borrow_mut().clear();
      optimize_ast(&ctx, &mut ast).unwrap();
      eval_program_with_ctx(&ctx, &ast).unwrap();
      assert_eq!(
        ctx.rendered_textures.len(),
        1,
        "expected a rendered texture on run {run}"
      );
    }
  }
}
