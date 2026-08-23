use std::rc::Rc;

use fxhash::FxHashMap;

use super::tex_kernels as kern;
use crate::{
  ArgRef, Callable, ErrorStack, EvalCtx, Mat4, RenderedTexture, Sym, TexKind, TexStorage,
  TextureHandle, TextureUsage, TextureWrap, Value, Vec2, Vec3, Vec4, EMPTY_KWARGS,
};

pub(crate) const MAX_TEXTURE_DIM: i64 = 8192;

impl TextureWrap {
  pub(crate) fn coord(self, c: i64, n: usize) -> usize {
    let n = n as i64;
    (match self {
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
}

impl TextureHandle {
  pub(crate) fn wrap_coord(&self, c: i64, n: usize) -> usize {
    self.wrap.coord(c, n)
  }

  pub(crate) fn texel(&self, x: i64, y: i64, chan: usize) -> f32 {
    let x = self.wrap_coord(x, self.width);
    let y = self.wrap_coord(y, self.height);
    self.texel_raw(x, y, chan)
  }
}

/// Dims must match exactly; channels must match or be broadcastable (1ch on either side
/// zips against every plane of the other).
fn zip_shape_check(a: &TextureHandle, b: &TextureHandle, op: &str) -> Result<(), ErrorStack> {
  let ch_ok = a.channels == b.channels || a.channels == 1 || b.channels == 1;
  if (a.width, a.height) != (b.width, b.height) || !ch_ok {
    return Err(ErrorStack::new(format!(
      "texture {op} texture requires matching dims and matching channels (or a 1-channel texture \
       on either side, which broadcasts); found {}x{}x{}ch vs {}x{}x{}ch",
      a.width, a.height, a.channels, b.width, b.height, b.channels
    )));
  }
  Ok(())
}

pub(crate) fn texture_zip(
  a: &TextureHandle,
  b: &TextureHandle,
  op: &str,
  f: impl Fn(f32, f32) -> f32,
) -> Result<Value, ErrorStack> {
  zip_shape_check(a, b, op)?;
  let (pa, pb) = (a.as_planes(), b.as_planes());
  let out_ch = a.channels.max(b.channels);
  let planes = (0..out_ch)
    .map(|c| {
      Rc::new(kern::zip_new(
        &pa[c.min(pa.len() - 1)],
        &pb[c.min(pb.len() - 1)],
        &f,
      ))
    })
    .collect();
  // Wrap/transform/filters follow the N-channel operand: under 1ch⊗Nch broadcast the
  // 1-channel side is a mask, and its placement is not the result's.
  let meta = if a.channels == out_ch { a } else { b };
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::planes(planes),
    channels: out_ch,
    mips: Default::default(),
    ..meta.clone()
  })))
}

/// Owned-handle stealing: when a chain temporary's handle and planes are uniquely held,
/// hand them back for in-place reuse. The caller must rebuild `storage` (fresh id) before
/// the handle escapes.
fn try_take_dense(
  t: Rc<TextureHandle>,
) -> Result<(TextureHandle, Vec<Rc<Vec<f32>>>), Rc<TextureHandle>> {
  match Rc::try_unwrap(t) {
    Ok(mut h) if h.is_dense() => {
      // Fresh placeholder id, not just emptied planes: a caller that forgets to re-stamp
      // would otherwise publish mutated pixels under the id the mip/host caches key on.
      let TexKind::Planes(planes) =
        std::mem::replace(&mut h.storage, TexStorage::planes(Vec::new())).kind
      else {
        unreachable!()
      };
      Ok((h, planes))
    }
    Ok(h) => Err(Rc::new(h)),
    Err(rc) => Err(rc),
  }
}

pub(crate) fn texture_zip_owned(
  a: Rc<TextureHandle>,
  b: Rc<TextureHandle>,
  op: &str,
  f: impl Fn(f32, f32) -> f32,
) -> Result<Value, ErrorStack> {
  zip_shape_check(&a, &b, op)?;
  if a.channels != b.channels {
    return texture_zip(&a, &b, op, f);
  }
  match try_take_dense(a) {
    Ok((mut h, planes)) => {
      let pb = b.as_planes();
      let planes = planes
        .into_iter()
        .zip(&pb)
        .map(|(pa, pb)| match Rc::try_unwrap(pa) {
          Ok(mut v) => {
            kern::zip_in_a(&mut v, pb, &f);
            Rc::new(v)
          }
          Err(pa) => Rc::new(kern::zip_new(&pa, pb, &f)),
        })
        .collect();
      h.storage = TexStorage::planes(planes);
      h.mips = Default::default();
      Ok(Value::Texture(Rc::new(h)))
    }
    Err(a) => match try_take_dense(b) {
      Ok((_, planes_b)) => {
        let pa = a.as_planes();
        let planes = pa
          .iter()
          .zip(planes_b)
          .map(|(pa, pb)| match Rc::try_unwrap(pb) {
            Ok(mut v) => {
              kern::zip_in_b(pa, &mut v, &f);
              Rc::new(v)
            }
            Err(pb) => Rc::new(kern::zip_new(pa, &pb, &f)),
          })
          .collect();
        Ok(Value::Texture(Rc::new(TextureHandle {
          storage: TexStorage::planes(planes),
          mips: Default::default(),
          ..(*a).clone()
        })))
      }
      Err(b) => texture_zip(&a, &b, op, f),
    },
  }
}

/// Elementwise map with the channel index available; one contiguous pass per plane.
pub(crate) fn texture_map_chan(t: &TextureHandle, f: impl Fn(f32, usize) -> f32) -> Value {
  let planes = t
    .as_planes()
    .iter()
    .enumerate()
    .map(|(c, p)| Rc::new(kern::map_new(p, |x| f(x, c))))
    .collect();
  Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::planes(planes),
    mips: Default::default(),
    ..t.clone()
  }))
}

pub(crate) fn texture_map_chan_owned(t: Rc<TextureHandle>, f: impl Fn(f32, usize) -> f32) -> Value {
  let (mut h, planes) = match try_take_dense(t) {
    Ok(x) => x,
    Err(t) => return texture_map_chan(&t, f),
  };
  let planes = planes
    .into_iter()
    .enumerate()
    .map(|(c, p)| match Rc::try_unwrap(p) {
      Ok(mut v) => {
        kern::map_in(&mut v, |x| f(x, c));
        Rc::new(v)
      }
      Err(p) => Rc::new(kern::map_new(&p, |x| f(x, c))),
    })
    .collect();
  h.storage = TexStorage::planes(planes);
  h.mips = Default::default();
  Value::Texture(Rc::new(h))
}

pub(crate) fn texture_map_unary(t: &TextureHandle, f: impl Fn(f32) -> f32) -> Value {
  texture_map_chan(t, |x, _| f(x))
}

pub(crate) fn texture_map_unary_owned(t: Rc<TextureHandle>, f: impl Fn(f32) -> f32) -> Value {
  texture_map_chan_owned(t, |x, _| f(x))
}

pub(crate) fn texture_scale_owned(t: Rc<TextureHandle>, s: f32) -> Value {
  texture_map_unary_owned(t, |x| x * s)
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

fn onech_from(t: &TextureHandle, plane: Vec<f32>) -> Value {
  Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::from_plane_vecs(vec![plane]),
    channels: 1,
    mips: Default::default(),
    ..t.clone()
  }))
}

fn same_shape_check(a: &TextureHandle, b: &TextureHandle, op: &str) -> Result<(), ErrorStack> {
  if (a.width, a.height, a.channels) != (b.width, b.height, b.channels) {
    return Err(ErrorStack::new(format!(
      "{op} requires textures with matching dims and channels; found {}x{}x{}ch vs {}x{}x{}ch",
      a.width, a.height, a.channels, b.width, b.height, b.channels
    )));
  }
  Ok(())
}

/// Per-texel dot product across channels -> 1ch.
pub(crate) fn texture_dot(a: &TextureHandle, b: &TextureHandle) -> Result<Value, ErrorStack> {
  same_shape_check(a, b, "`dot` of two textures")?;
  let (pa, pb) = (a.as_planes(), b.as_planes());
  let mut acc = kern::zip_new(&pa[0], &pb[0], |x, y| x * y);
  for c in 1..a.channels {
    kern::mul_acc(&mut acc, &pa[c], &pb[c]);
  }
  Ok(onech_from(a, acc))
}

/// Per-texel vector length across channels -> 1ch.
pub(crate) fn texture_len(t: &TextureHandle) -> Value {
  let p = t.as_planes();
  let mut acc = kern::map_new(&p[0], |x| x * x);
  for c in 1..t.channels {
    kern::mul_acc(&mut acc, &p[c], &p[c]);
  }
  kern::map_in(&mut acc, f32::sqrt);
  onech_from(t, acc)
}

/// Per-texel vector normalize across channels.
pub(crate) fn texture_normalize_vec(t: &TextureHandle) -> Value {
  let p = t.as_planes();
  let mut len = kern::map_new(&p[0], |x| x * x);
  for c in 1..t.channels {
    kern::mul_acc(&mut len, &p[c], &p[c]);
  }
  kern::map_in(&mut len, f32::sqrt);
  let planes = p
    .iter()
    .map(|pc| Rc::new(kern::zip_new(pc, &len, |x, l| x / l)))
    .collect();
  Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::planes(planes),
    mips: Default::default(),
    ..t.clone()
  }))
}

/// Per-texel euclidean distance across channels -> 1ch.
pub(crate) fn texture_distance(a: &TextureHandle, b: &TextureHandle) -> Result<Value, ErrorStack> {
  same_shape_check(a, b, "`distance` of two textures")?;
  let (pa, pb) = (a.as_planes(), b.as_planes());
  let mut acc = kern::zip_new(&pa[0], &pb[0], |x, y| (x - y) * (x - y));
  for c in 1..a.channels {
    kern::diff_sq_acc(&mut acc, &pa[c], &pb[c]);
  }
  kern::map_in(&mut acc, f32::sqrt);
  Ok(onech_from(a, acc))
}

/// 3-way elementwise lerp; any operand may be 1ch (broadcast), dims must match.
pub(crate) fn texture_lerp(
  t: &TextureHandle,
  a: &TextureHandle,
  b: &TextureHandle,
) -> Result<Value, ErrorStack> {
  let out_ch = t.channels.max(a.channels).max(b.channels);
  let shape_ok = |x: &TextureHandle| {
    (x.width, x.height) == (a.width, a.height) && (x.channels == out_ch || x.channels == 1)
  };
  if !shape_ok(t) || !shape_ok(a) || !shape_ok(b) {
    return Err(ErrorStack::new(format!(
      "`lerp` of textures requires matching dims and matching channels (or 1-channel broadcast); \
       found t={}x{}x{}ch, a={}x{}x{}ch, b={}x{}x{}ch",
      t.width, t.height, t.channels, a.width, a.height, a.channels, b.width, b.height, b.channels
    )));
  }
  let (pt, pa, pb) = (t.as_planes(), a.as_planes(), b.as_planes());
  let planes = (0..out_ch)
    .map(|c| {
      Rc::new(kern::zip3_new(
        &pa[c.min(pa.len() - 1)],
        &pb[c.min(pb.len() - 1)],
        &pt[c.min(pt.len() - 1)],
        |x, y, t| x + (y - x) * t,
      ))
    })
    .collect();
  let meta = [a, b, t]
    .into_iter()
    .find(|x| x.channels == out_ch)
    .unwrap();
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::planes(planes),
    channels: out_ch,
    mips: Default::default(),
    ..meta.clone()
  })))
}

/// `vecN(...)` over texture/scalar components: each texture must be 1ch with matching dims;
/// scalars become filled planes. Texture planes are shared zero-copy.
pub(crate) fn texture_construct(name: &str, comps: &[&Value]) -> Result<Value, ErrorStack> {
  let mut meta: Option<&TextureHandle> = None;
  for v in comps {
    if let Value::Texture(t) = v {
      if t.channels != 1 {
        return Err(ErrorStack::new(format!(
          "texture components of `{name}` must be 1-channel; found {} channel(s)",
          t.channels
        )));
      }
      if let Some(m) = meta {
        if (m.width, m.height) != (t.width, t.height) {
          return Err(ErrorStack::new(format!(
            "texture components of `{name}` must have matching dims; found {}x{} vs {}x{}",
            m.width, m.height, t.width, t.height
          )));
        }
      } else {
        meta = Some(t);
      }
    }
  }
  let Some(meta) = meta else {
    return Err(ErrorStack::new(format!(
      "`{name}` over textures requires at least one texture component"
    )));
  };
  let n = meta.width * meta.height;
  let planes = comps
    .iter()
    .map(|v| match v {
      Value::Texture(t) => Ok(t.as_planes()[0].clone()),
      v => Ok(Rc::new(vec![
        v.as_float().ok_or_else(|| {
          ErrorStack::new(format!(
            "components of `{name}` must be numeric or 1-channel textures, found: {v:?}"
          ))
        })?;
        n
      ])),
    })
    .collect::<Result<_, ErrorStack>>()?;
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::planes(planes),
    channels: comps.len(),
    mips: Default::default(),
    ..meta.clone()
  })))
}

/// `vecN(t)` splat: a 1ch texture replicated to N shared planes, zero-copy.
pub(crate) fn texture_splat(name: &str, t: &TextureHandle, n: usize) -> Result<Value, ErrorStack> {
  if t.channels != 1 {
    return Err(ErrorStack::new(format!(
      "`{name}(texture)` splat requires a 1-channel texture; found {} channel(s)",
      t.channels
    )));
  }
  let plane = t.as_planes()[0].clone();
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::planes(vec![plane; n]),
    channels: n,
    mips: Default::default(),
    ..t.clone()
  })))
}

pub(crate) fn texture_zip_vec_owned(
  t: Rc<TextureHandle>,
  v: &[f32],
  op: &str,
  f: impl Fn(f32, f32) -> f32,
) -> Result<Value, ErrorStack> {
  if t.channels != v.len() {
    return texture_zip_vec(&t, v, op, f);
  }
  Ok(texture_map_chan_owned(t, |x, c| f(x, v[c])))
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
  for (c, p) in t.as_planes().iter().enumerate() {
    for &x in p.iter() {
      fold(c, x);
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

/// Toroidal shift: `out[x, y] = in[(x - dx) mod w, (y - dy) mod h]`, so positive offsets
/// move content toward +x/+y. Two memcpys per row per plane.
pub(crate) fn roll_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let dx = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
  let dy = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
  let t = arg_refs[2].resolve(args, kwargs).as_texture().unwrap();
  let (w, h) = (t.width, t.height);
  let dx = dx.rem_euclid(w as i64) as usize;
  let dy = dy.rem_euclid(h as i64) as usize;
  if dx == 0 && dy == 0 {
    return Ok(Value::Texture(Rc::clone(t)));
  }
  let planes = t
    .as_planes()
    .iter()
    .map(|p| {
      let mut out = Vec::with_capacity(w * h);
      for y in 0..h {
        let src = &p[((y + h - dy) % h) * w..][..w];
        out.extend_from_slice(&src[w - dx..]);
        out.extend_from_slice(&src[..w - dx]);
      }
      out
    })
    .collect();
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::from_plane_vecs(planes),
    mips: Default::default(),
    ..(**t).clone()
  })))
}

/// `sample(texture, uv, filter, wrap)`: one continuous-coordinate read. The same kernel
/// backs the vectorizer's gather step, which is what keeps the two paths bit-identical.
pub(crate) fn sample_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let t = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let uv = *arg_refs[1].resolve(args, kwargs).as_vec2().unwrap();
  let filter = kern::SampleFilter::from_name(arg_refs[2].resolve(args, kwargs).as_str().unwrap())?;
  let wrap = match arg_refs[3].resolve(args, kwargs) {
    Value::Nil => t.wrap,
    v => TextureWrap::from_name(v.as_str().unwrap())?,
  };
  let mut px = [0f32; 4];
  let (planes, origin, x_pitch, y_pitch) = t.gather_parts();
  let src = kern::GatherSrc {
    planes: &planes,
    w: t.width,
    h: t.height,
    origin,
    x_pitch,
    y_pitch,
    wrap,
  };
  kern::sample_texel(&src, filter, uv.x, uv.y, &mut px);
  Ok(channels_value(px, t.channels))
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

pub(crate) fn texture_std_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let t = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let stats = t.stats();
  let mut out = [0f32; 4];
  for c in 0..t.channels {
    out[c] = stats.channels[c].std;
  }
  Ok(channels_value(out, t.channels))
}

pub(crate) fn texture_quantile_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let t = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let q = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
  if !(0. ..=1.).contains(&q) {
    return Err(ErrorStack::new(format!(
      "texture_quantile: `q` must be in [0, 1], got {q}"
    )));
  }
  let stats = t.stats();
  let mut out = [0f32; 4];
  for c in 0..t.channels {
    out[c] = stats.channels[c].quantile(q);
  }
  Ok(channels_value(out, t.channels))
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

  if let Some(res) = crate::tex_vectorize::try_vectorized_texture(ctx, generator, w, h, wrap) {
    if !ctx.tex_vectorize.verify.get() {
      return res;
    }
    let vec_val = res?;
    let scalar_val = texture_generate_scalar(ctx, generator, w, h, wrap)?;
    crate::tex_vectorize::assert_bit_identical(&vec_val, &scalar_val)?;
    return Ok(vec_val);
  }
  texture_generate_scalar(ctx, generator, w, h, wrap)
}

fn texture_generate_scalar(
  ctx: &EvalCtx,
  generator: &Rc<Callable>,
  w: usize,
  h: usize,
  wrap: TextureWrap,
) -> Result<Value, ErrorStack> {
  let mut planes: Vec<Vec<f32>> = Vec::new();
  let mut channels = 0usize;
  for y in 0..h {
    for x in 0..w {
      let uv = Vec2::new((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
      let out = ctx
        .invoke_callable(
          generator,
          &[Value::Vec2(uv), Value::Int(x as i64), Value::Int(y as i64)],
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
        planes = (0..n).map(|_| Vec::with_capacity(w * h)).collect();
      } else if n != channels {
        return Err(ErrorStack::new(
          "`generator` callable in `texture` returned a mix of float/vec2/vec3/vec4 values",
        ));
      }
      for (c, plane) in planes.iter_mut().enumerate() {
        plane.push(px[c]);
      }
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::from_plane_vecs(planes),
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
  if let Some(res) = crate::tex_vectorize::try_vectorized_map(ctx, cb, tex) {
    if !ctx.tex_vectorize.verify.get() {
      return res;
    }
    let vec_val = res?;
    let scalar_val = map_texture_scalar(ctx, cb, tex)?;
    crate::tex_vectorize::assert_bit_identical(&vec_val, &scalar_val)?;
    return Ok(vec_val);
  }
  map_texture_scalar(ctx, cb, tex)
}

fn map_texture_scalar(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  tex: &TextureHandle,
) -> Result<Value, ErrorStack> {
  let (w, h, ch) = (tex.width, tex.height, tex.channels);
  let src = tex.as_planes();
  let mut planes: Vec<Vec<f32>> = Vec::new();
  let mut out_ch = 0usize;
  for y in 0..h {
    for x in 0..w {
      let i = y * w + x;
      let mut v = [0f32; 4];
      for (c, out) in v[..ch].iter_mut().enumerate() {
        *out = src[c][i];
      }
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
        planes = (0..n).map(|_| Vec::with_capacity(w * h)).collect();
      } else if n != out_ch {
        return Err(ErrorStack::new(
          "callable passed to `map` over texture returned a mix of float/vec2/vec3/vec4 values",
        ));
      }
      for (c, plane) in planes.iter_mut().enumerate() {
        plane.push(px[c]);
      }
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    channels: out_ch,
    storage: TexStorage::from_plane_vecs(planes),
    mips: Default::default(),
    ..tex.clone()
  })))
}

pub(crate) fn texture_zip_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let cb = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
  let seq = arg_refs[1].resolve(args, kwargs).as_sequence().unwrap();

  let mut texs: Vec<Rc<TextureHandle>> = Vec::new();
  for (i, val) in seq.consume(ctx).enumerate() {
    let val = val.map_err(|err| err.wrap("Error produced by `textures` seq in `texture_zip`"))?;
    let Value::Texture(tex) = val else {
      return Err(ErrorStack::new(format!(
        "Expected texture at index {i} of `textures` in `texture_zip`, found: {val:?}"
      )));
    };
    if let Some(first) = texs.first() {
      if (tex.width, tex.height) != (first.width, first.height) {
        return Err(ErrorStack::new(format!(
          "All textures passed to `texture_zip` must have matching dims; index 0 is {}x{} but \
           index {i} is {}x{}",
          first.width, first.height, tex.width, tex.height
        )));
      }
    }
    texs.push(tex);
  }
  if texs.is_empty() {
    return Err(ErrorStack::new(
      "`texture_zip` requires at least one texture in `textures`",
    ));
  }

  let texs: Vec<&TextureHandle> = texs.iter().map(|t| &**t).collect();
  if let Some(res) = crate::tex_vectorize::try_vectorized_zip(ctx, cb, &texs) {
    if !ctx.tex_vectorize.verify.get() {
      return res;
    }
    let vec_val = res?;
    let scalar_val = zip_texture_scalar(ctx, cb, &texs)?;
    crate::tex_vectorize::assert_bit_identical(&vec_val, &scalar_val)?;
    return Ok(vec_val);
  }
  zip_texture_scalar(ctx, cb, &texs)
}

/// Also the differential oracle for the vectorized path, so it must stay a faithful
/// per-texel interpretation of the same body.
fn zip_texture_scalar(
  ctx: &EvalCtx,
  cb: &Rc<Callable>,
  texs: &[&TextureHandle],
) -> Result<Value, ErrorStack> {
  let (w, h) = (texs[0].width, texs[0].height);
  let srcs: Vec<(Vec<Rc<Vec<f32>>>, usize)> =
    texs.iter().map(|t| (t.as_planes(), t.channels)).collect();
  let n = texs.len();
  let mut cb_args = vec![Value::Nil; n + 3];
  let mut planes: Vec<Vec<f32>> = Vec::new();
  let mut out_ch = 0usize;
  for y in 0..h {
    for x in 0..w {
      let i = y * w + x;
      for (arg, (src, ch)) in cb_args.iter_mut().zip(&srcs) {
        let mut v = [0f32; 4];
        for (c, out) in v[..*ch].iter_mut().enumerate() {
          *out = src[c][i];
        }
        *arg = channels_value(v, *ch);
      }
      cb_args[n] = Value::Vec2(Vec2::new(
        (x as f32 + 0.5) / w as f32,
        (y as f32 + 0.5) / h as f32,
      ));
      cb_args[n + 1] = Value::Int(x as i64);
      cb_args[n + 2] = Value::Int(y as i64);
      let res = ctx
        .invoke_callable(cb, &cb_args, EMPTY_KWARGS)
        .map_err(|err| err.wrap("Error produced by callable passed to `texture_zip`"))?;
      let (px, out_n) = pixel_from_value(&res).ok_or_else(|| {
        ErrorStack::new(format!(
          "Expected float, vec2, vec3, or vec4 from callable passed to `texture_zip`, found: \
           {res:?}"
        ))
      })?;
      if out_ch == 0 {
        out_ch = out_n;
        planes = (0..out_n).map(|_| Vec::with_capacity(w * h)).collect();
      } else if out_n != out_ch {
        return Err(ErrorStack::new(
          "callable passed to `texture_zip` returned a mix of float/vec2/vec3/vec4 values",
        ));
      }
      for (c, plane) in planes.iter_mut().enumerate() {
        plane.push(px[c]);
      }
    }
  }

  // Metadata follows input 0; the other inputs contribute pixels only.
  Ok(Value::Texture(Rc::new(TextureHandle {
    channels: out_ch,
    storage: TexStorage::from_plane_vecs(planes),
    mips: Default::default(),
    ..texs[0].clone()
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

/// Owned plane copies with RGBA premultiplied so transparent texels' RGB doesn't bleed
/// into visible ones under filtering; a plain copy below 4 channels.
pub(crate) fn premultiplied_planes(tex: &TextureHandle) -> Vec<Vec<f32>> {
  let mut planes: Vec<Vec<f32>> = tex.as_planes().iter().map(|p| p.to_vec()).collect();
  if tex.channels == 4 {
    let (rgb, a) = planes.split_at_mut(3);
    for p in rgb {
      for (v, &a) in p.iter_mut().zip(a[0].iter()) {
        *v *= a;
      }
    }
  }
  planes
}

pub(crate) fn unpremultiply_planes(planes: &mut [Vec<f32>]) {
  debug_assert!(planes.len() >= 4);
  let (rgb, a) = planes.split_at_mut(3);
  for p in rgb {
    for (v, &a) in p.iter_mut().zip(a[0].iter()) {
      if a > 1e-8 {
        *v /= a;
      }
    }
  }
}

/// Separable gaussian; caller guarantees `sigma > 0`.
pub(crate) fn blur_tex(sigma: f32, tex: &TextureHandle) -> TextureHandle {
  let (w, h, ch) = (tex.width, tex.height, tex.channels);
  let wrap = tex.wrap;
  let half = ((sigma * 3.).ceil() as i64).max(1);
  let mut weights = Vec::with_capacity(half as usize + 1);
  for i in 0..=half {
    weights.push((-((i * i) as f32) / (2. * sigma * sigma)).exp());
  }
  let norm = weights[0] + 2. * weights[1..].iter().sum::<f32>();
  for wt in &mut weights {
    *wt /= norm;
  }

  // Taps run tap-major (one weight across a whole run) rather than pixel-major so the
  // inner loop is a flat stride-1 accumulate. Each output still accumulates its taps in
  // ascending `i`, so results are bit-identical to the pixel-major form.
  let pass = |src: &[f32], dx: i64, dy: i64| -> Vec<f32> {
    let horiz = dx == 1;
    let tap = |x: i64, y: i64| src[wrap.coord(y, h) * w + wrap.coord(x, w)];
    let edge = |out: &mut [f32], x: i64, y: i64| {
      let mut acc = weights[0] * tap(x, y);
      for i in 1..=half {
        acc += weights[i as usize] * (tap(x - i * dx, y - i * dy) + tap(x + i * dx, y + i * dy));
      }
      out[y as usize * w + x as usize] = acc;
    };

    let mut out = vec![0f32; w * h];
    // Interior = positions whose full kernel is in bounds; empty when the axis is shorter
    // than the kernel, which then leaves every position on the wrapped path.
    let interior = |n: usize| {
      let lo = (half as usize).min(n);
      (lo, (n as i64 - half).max(lo as i64) as usize)
    };

    if horiz {
      let (x0, x1) = interior(w);
      for y in 0..h {
        let (s, o) = (&src[y * w..(y + 1) * w], &mut out[y * w..(y + 1) * w]);
        for (x, slot) in o[x0..x1].iter_mut().enumerate() {
          *slot = weights[0] * s[x0 + x];
        }
        for i in 1..=half as usize {
          let wt = weights[i];
          for (x, slot) in o[x0..x1].iter_mut().enumerate() {
            *slot += wt * (s[x0 + x - i] + s[x0 + x + i]);
          }
        }
        for x in (0..x0).chain(x1..w) {
          edge(&mut out, x as i64, y as i64);
        }
      }
    } else {
      let (y0, y1) = interior(h);
      for y in y0..y1 {
        let o = &mut out[y * w..(y + 1) * w];
        for (x, slot) in o.iter_mut().enumerate() {
          *slot = weights[0] * src[y * w + x];
        }
        for i in 1..=half as usize {
          let wt = weights[i];
          let (up, dn) = (&src[(y - i) * w..], &src[(y + i) * w..]);
          for (x, slot) in o.iter_mut().enumerate() {
            *slot += wt * (up[x] + dn[x]);
          }
        }
      }
      for y in (0..y0).chain(y1..h) {
        for x in 0..w {
          edge(&mut out, x as i64, y as i64);
        }
      }
    }
    out
  };

  let mut planes = premultiplied_planes(tex);
  for p in &mut planes {
    let mid = pass(p, 1, 0);
    *p = pass(&mid, 0, 1);
  }
  if ch == 4 {
    unpremultiply_planes(&mut planes);
  }
  TextureHandle {
    storage: TexStorage::from_plane_vecs(planes),
    mips: Default::default(),
    ..tex.clone()
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

  #[inline(always)]
  fn encode_normal(dx: f32, dy: f32) -> [f32; 3] {
    let inv_len = 1. / (dx * dx + dy * dy + 1.).sqrt();
    [
      -dx * inv_len * 0.5 + 0.5,
      -dy * inv_len * 0.5 + 0.5,
      inv_len * 0.5 + 0.5,
    ]
  }

  let tex = tex.dense_clone();
  let (w, h) = (tex.width, tex.height);
  let src: &[f32] = &tex.planes().expect("dense_clone yields planar storage")[0];
  let mut planes: Vec<Vec<f32>> = (0..3).map(|_| vec![0f32; w * h]).collect();
  let put = |o: usize, n: [f32; 3], planes: &mut [Vec<f32>]| {
    for (p, v) in planes.iter_mut().zip(n) {
      p[o] = v;
    }
  };

  // Interior taps can't leave the texture, so they index the plane directly instead of
  // paying the wrap funnel's rem_euclid on all four neighbors.
  if w >= 3 && h >= 3 {
    for y in 1..h - 1 {
      for x in 1..w - 1 {
        let o = y * w + x;
        let dx = (src[o + 1] - src[o - 1]) * 0.5 * strength;
        let dy = (src[o + w] - src[o - w]) * 0.5 * strength;
        put(o, encode_normal(dx, dy), &mut planes);
      }
    }
  }
  let interior = |x: usize, y: usize| w >= 3 && h >= 3 && x > 0 && y > 0 && x < w - 1 && y < h - 1;
  for y in 0..h {
    for x in 0..w {
      if interior(x, y) {
        continue;
      }
      let (xi, yi) = (x as i64, y as i64);
      let dx = (tex.texel(xi + 1, yi, 0) - tex.texel(xi - 1, yi, 0)) * 0.5 * strength;
      let dy = (tex.texel(xi, yi + 1, 0) - tex.texel(xi, yi - 1, 0)) * 0.5 * strength;
      put(y * w + x, encode_normal(dx, dy), &mut planes);
    }
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    channels: 3,
    storage: TexStorage::from_plane_vecs(planes),
    mips: Default::default(),
    ..tex.clone()
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
    let val =
      val.map_err(|err| err.wrap("Error produced by `slices` seq in `render_texture_stack`"))?;
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
          first.width,
          first.height,
          first.channels,
          first.wrap,
          tex.width,
          tex.height,
          tex.channels,
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
  use super::*;
  use crate::{
    eval_program_with_ctx, optimizer::optimize_ast, parse_and_eval_program, parse_program_src,
    EvalCtx,
  };

  fn test_tex(w: usize, h: usize, ch: usize) -> Rc<TextureHandle> {
    let planes = (0..ch)
      .map(|c| (0..w * h).map(|i| (i * ch + c) as f32).collect())
      .collect();
    Rc::new(TextureHandle {
      storage: TexStorage::from_plane_vecs(planes),
      width: w,
      height: h,
      channels: ch,
      wrap: TextureWrap::Repeat,
      min_filter: None,
      mag_filter: None,
      format: None,
      transform: Mat4::identity(),
      mips: Default::default(),
    })
  }

  fn plane_ptr(v: &Value, c: usize) -> *const f32 {
    v.as_texture().unwrap().planes().unwrap()[c].as_ptr()
  }

  #[test]
  fn owned_ops_steal_unique_buffers() {
    // uniquely-owned temporary: buffer reused, identity fresh
    let t = test_tex(4, 4, 2);
    let (ptr, old_id) = (t.planes().unwrap()[0].as_ptr(), t.storage_id());
    let out = texture_map_unary_owned(t, |x| x + 1.);
    assert_eq!(plane_ptr(&out, 0), ptr);
    assert_ne!(out.as_texture().unwrap().storage_id(), old_id);
    assert_eq!(out.as_texture().unwrap().as_planes()[1][3], 8.);

    // shared handle: untouched, new buffers
    let t = test_tex(4, 4, 1);
    let keep = Rc::clone(&t);
    let out = texture_map_unary_owned(t, |x| x + 1.);
    assert_ne!(plane_ptr(&out, 0), keep.planes().unwrap()[0].as_ptr());
    assert_eq!(keep.as_planes()[0][1], 1.);

    // zip: lhs shared, rhs unique -> rhs buffer reused, metadata/orientation from lhs
    let a = test_tex(2, 2, 1);
    let a_keep = Rc::clone(&a);
    let b = test_tex(2, 2, 1);
    let b_ptr = b.planes().unwrap()[0].as_ptr();
    let out = texture_zip_owned(a, b, "-", |x, y| x - y).unwrap();
    assert_eq!(plane_ptr(&out, 0), b_ptr);
    assert!(out.as_texture().unwrap().as_planes()[0]
      .iter()
      .all(|&x| x == 0.));
    assert_eq!(a_keep.as_planes()[0][2], 2.);
  }

  #[test]
  fn texture_overload_shape_errors() {
    for (src, needle) in [
      (r#"v3(texture(2, 2, |uv| v2(0., 0.)), 1., 1.)"#, "1-channel"),
      (
        r#"v2(texture(2, 2, |uv| 0.), texture(4, 2, |uv| 0.))"#,
        "matching dims",
      ),
      (r#"v4(texture(2, 2, |uv| v3(0., 0., 0.)))"#, "1-channel"),
      (
        r#"dot(texture(2, 2, |uv| v3(0., 0., 0.)), texture(2, 2, |uv| 0.))"#,
        "matching dims and channels",
      ),
    ] {
      let err = parse_and_eval_program(src).unwrap_err();
      assert!(err.to_string().contains(needle), "{src}: {err}");
    }
  }

  #[test]
  fn texture_zip_1ch_broadcast() {
    let ctx = parse_and_eval_program(
      r#"
rgb = texture(2, 2, |uv| v3(1., 2., 4.))
mask = texture(2, 2, |uv, x, y| float(x))
(rgb * mask) | render_texture(name="masked")
(mask + rgb) | render_texture(name="summed")
"#,
    )
    .unwrap();
    let rendered = ctx.rendered_textures.into_inner();
    let masked = &rendered[0].texture;
    assert_eq!(masked.channels, 3);
    assert_eq!(masked.as_interleaved()[0..6], [0., 0., 0., 1., 2., 4.]);
    assert_eq!(rendered[1].texture.as_interleaved()[3..6], [2., 3., 5.]);

    let err = parse_and_eval_program(
      r#"texture(2, 2, |uv| v3(0., 0., 0.)) + texture(2, 2, |uv| v2(0., 0.))"#,
    )
    .unwrap_err();
    assert!(err.to_string().contains("matching dims"), "{err}");
  }

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
    assert_eq!(tex.as_interleaved().len(), 8);
    for y in 0..2 {
      for x in 0..4 {
        assert_eq!(tex.as_interleaved()[y * 4 + x], (x as f32 + 0.5) / 4.);
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
    let ix = get("ix").as_interleaved().to_vec();
    for y in 0..2 {
      for x in 0..4 {
        let base = (y * 4 + x) * 2;
        assert_eq!((ix[base], ix[base + 1]), (x as f32, y as f32));
      }
    }
    // A closure declaring only `uv` still works; the extra args are dropped.
    assert_eq!(get("uv_only").as_interleaved()[0], 0.125);
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
    assert_eq!(
      (t.texture.width, t.texture.height, t.texture.channels),
      (4, 2, 1)
    );
    assert_eq!(t.texture.as_interleaved()[0], 0.5 / 4.);
    assert_eq!(t.extra_slices[1].as_interleaved()[0], 1.);

    let err =
      parse_and_eval_program(r#"[texture(2, 2, |uv| 0.)] | render_texture_stack(name="x")"#)
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
    let src =
      r#"[texture(2, 2, |uv| 0.), texture(2, 2, |uv| 1.)] | render_texture_stack(name="s")"#;
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
      assert_eq!(
        rendered[0].source_module.as_deref(),
        Some("textab:root"),
        "run {run}"
      );
      assert_eq!(
        rendered[0].texture.format,
        Some(crate::TextureFormat::R32F),
        "run {run}"
      );
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
        assert_eq!(rgb.as_interleaved()[base], (x as f32 + 0.5) / 4.);
        assert_eq!(rgb.as_interleaved()[base + 1], (y as f32 + 0.5) / 2.);
        assert_eq!(rgb.as_interleaved()[base + 2], (x + y) as f32);
      }
    }

    let doubled = &rendered[1].texture;
    assert_eq!(doubled.channels, 1);
    assert_eq!(doubled.as_interleaved()[1], 2. * (1.5 / 4.));
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
    assert!(wrapped.as_interleaved()[7] > 0.);
    assert_eq!(clamped.as_interleaved()[7], 0.);

    let flat_n = &rendered[2].texture;
    assert_eq!(flat_n.channels, 3);
    for px in flat_n.as_interleaved().chunks(3) {
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
    let px = |i: usize| rendered[i].texture.as_interleaved();
    assert_eq!(px(0)[0], 0.25 + 1.);
    assert_eq!(px(1)[0], 0.25);
    assert_eq!(px(1)[1], 0.75);
    assert_eq!(px(2)[1], 0.75);
    let colored = &rendered[3].texture;
    assert_eq!(colored.channels, 3);
    assert!(colored.as_interleaved()[0] < colored.as_interleaved()[3]);

    let err =
      parse_and_eval_program(r#"texture(2, 2, |uv| 0.) + texture(4, 2, |uv| 0.)"#).unwrap_err();
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
    assert!(alpha.as_interleaved().iter().all(|&p| p == 0.5));

    let rgba = &rendered[1].texture;
    assert_eq!((rgba.width, rgba.height, rgba.channels), (2, 2, 4));
    assert_eq!(rgba.as_interleaved()[0..4], [0.25, 0.25, 1., 0.5]);
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

  /// Under 1ch⊗Nch broadcast the 1-channel operand is a mask; wrap/transform/filters must
  /// come from the N-channel side whichever order it was written in. Pixel-hash goldens
  /// can't see this.
  #[test]
  fn broadcast_takes_metadata_from_the_n_channel_side() {
    let src = r#"
mask = texture(8, 8, |uv| uv.x)
stamp = texture(8, 8, |uv| v3(uv.x, uv.y, 0.5), wrap="clamp")
(mask * stamp) | render_texture(name="a")
(stamp * mask) | render_texture(name="b")
lerp(mask, mask, stamp) | render_texture(name="c")
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    for rt in ctx.rendered_textures.borrow().iter() {
      assert_eq!(rt.texture.channels, 3, "{}", rt.name);
      assert_eq!(rt.texture.wrap, TextureWrap::Clamp, "{}", rt.name);
    }
  }
}
