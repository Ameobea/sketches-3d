//! Atlas bakes: evaluate something per texel of a mesh's UV layout and return it as a plain
//! texture. `bake_texture` runs a user field, `bake_attr` rasterizes a vertex attribute,
//! `bake_sdf` a (Lipschitz-pruned) signed distance to a mask mesh, `bake_triplanar` projects a
//! tiling texture the way the runtime triplanar shader does, and `bake_decal` projects a stamp
//! through a world transform.

use std::{
  cell::{OnceCell, RefCell},
  rc::Rc,
};

use arrayvec::ArrayVec;
use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{Interp, Vec3},
  LinkedMesh,
};
use nalgebra::{Matrix3, Matrix4, Vector4};

use super::spectral_noise::fm;
use super::{
  blit::get_or_build_mips,
  texture::{zip_textures, MAX_TEXTURE_DIM},
  vertex_attrs::available,
};
use crate::{
  ast::DestructurePattern,
  mesh_ops::{
    bake_ao::{AoParams, Sampler},
    bake_texture::UvRaster,
  },
  ArgRef, Callable, ErrorStack, EvalCtx, Mat4, MeshHandle, MipLevel, Sym, TexStorage,
  TextureHandle, TextureWrap, Value,
};

/// Byte budget across cached atlases; the newest entry always stays, and the texture memory
/// manager may evict the rest under pressure.
const ATLAS_CACHE_BYTES: usize = 320 << 20;
/// `exp(-13.8)` ≈ 1e-6: blur taps past this contribute nothing.
const BLUR_CUTOFF: f32 = 13.8;

/// One mesh's rasterized atlas plus its lazily built per-vertex geometry (indexed like
/// `raster.verts`), shared by every bake on the same mesh and dims within a run. Texel
/// positions and normals are interpolated on demand rather than stored: they would triple the
/// atlas's footprint. Index 0 = local space, 1 = world space.
pub struct AtlasCache {
  pub raster: UvRaster,
  pos: [OnceCell<Rc<Vec<Vec3>>>; 2],
  normal: [OnceCell<Rc<Vec<Vec3>>>; 2],
}

pub(crate) type AtlasCaches = RefCell<
  Vec<(
    Rc<LinkedMesh<()>>,
    (usize, usize, usize, Matrix4<f32>),
    Rc<AtlasCache>,
  )>,
>;

impl crate::tex_mem::Evict for AtlasCaches {
  fn evict_one(&self) -> bool {
    self.try_borrow_mut().is_ok_and(|mut c| {
      !c.is_empty() && {
        c.remove(0);
        true
      }
    })
  }
}

fn dims(builtin: &str, w: &Value, h: &Value) -> Result<(usize, usize), ErrorStack> {
  let (w, h) = (w.as_int().unwrap(), h.as_int().unwrap());
  if w < 1 || h < 1 || w > MAX_TEXTURE_DIM || h > MAX_TEXTURE_DIM {
    return Err(ErrorStack::new(format!(
      "Invalid `{builtin}` dims {w}x{h}; expected 1..={MAX_TEXTURE_DIM} per side"
    )));
  }
  Ok((w as usize, h as usize))
}

pub(crate) fn ao_intensity(builtin: &str, v: &Value) -> Result<f32, ErrorStack> {
  let k = v.as_float().unwrap();
  if k < 0. {
    return Err(ErrorStack::new(format!(
      "`{builtin}` `intensity` must be >= 0"
    )));
  }
  Ok(k)
}

fn atlas_texture(w: usize, h: usize, planes: Vec<Vec<f32>>) -> TextureHandle {
  TextureHandle {
    channels: planes.len(),
    storage: TexStorage::from_plane_vecs(planes),
    width: w,
    height: h,
    wrap: TextureWrap::Clamp,
    min_filter: None,
    mag_filter: None,
    format: None,
    transform: Mat4::identity(),
    mips: Default::default(),
  }
}

fn renormalize(planes: &mut [Vec<f32>]) {
  for i in 0..planes[0].len() {
    let len = planes.iter().map(|p| p[i] * p[i]).sum::<f32>().sqrt();
    if len > 1e-12 {
      for p in planes.iter_mut() {
        p[i] /= len;
      }
    }
  }
}

fn atlas_for(
  ctx: &EvalCtx,
  mesh: &MeshHandle,
  w: usize,
  h: usize,
  dilate: usize,
) -> Result<Rc<AtlasCache>, ErrorStack> {
  // The transform is part of the key because the cached world-space buffers depend on it.
  let key = (w, h, dilate, mesh.transform);
  {
    let mut cache = ctx.uv_rasters.borrow_mut();
    if let Some(pos) = cache
      .iter()
      .position(|(m, k, _)| Rc::ptr_eq(m, &mesh.mesh) && *k == key)
    {
      let hit = cache.remove(pos);
      let r = Rc::clone(&hit.2);
      cache.push(hit);
      return Ok(r);
    }
  }
  // Built with the cache released, so the memory manager can evict from it to make room.
  let r = Rc::new(AtlasCache {
    raster: UvRaster::new(&mesh.mesh, w, h, dilate)?,
    pos: Default::default(),
    normal: Default::default(),
  });
  let mut cache = ctx.uv_rasters.borrow_mut();
  let mut held = w * h * UvRaster::TEXEL_BYTES;
  let keep_from = cache
    .iter()
    .rposition(|(_, (cw, ch, ..), _)| {
      held += cw * ch * UvRaster::TEXEL_BYTES;
      held > ATLAS_CACHE_BYTES
    })
    .map_or(0, |i| i + 1);
  cache.drain(..keep_from);
  cache.push((Rc::clone(&mesh.mesh), key, Rc::clone(&r)));
  Ok(r)
}

impl AtlasCache {
  fn positions(&self, mesh: &MeshHandle, fr: &Frame) -> Rc<Vec<Vec3>> {
    Rc::clone(self.pos[fr.world as usize].get_or_init(|| {
      let m: &LinkedMesh<()> = &mesh.mesh;
      Rc::new(
        self
          .raster
          .verts
          .iter()
          .map(|&k| fr.pos(m.vertices[k].position))
          .collect(),
      )
    }))
  }

  /// Smooth per-vertex normals: authored shading normals when complete, else displacement
  /// normals (computed on a clone when missing).
  fn normals(&self, mesh: &MeshHandle, fr: &Frame) -> Rc<Vec<Vec3>> {
    Rc::clone(self.normal[fr.world as usize].get_or_init(|| {
      let src: &LinkedMesh<()> = &mesh.mesh;
      let complete_shading =
        !src.shading_normals.is_empty() && src.shading_normals.len() == src.vertices.len();
      let owned;
      let m = if complete_shading
        || src
          .vertices
          .keys()
          .next()
          .is_none_or(|k| src.displacement_normal(k).is_some())
      {
        src
      } else {
        owned = {
          let mut c = src.clone();
          c.compute_vertex_displacement_normals();
          c
        };
        &owned
      };
      let of = |k| {
        fr.normal(if complete_shading {
          m.shading_normals[k]
        } else {
          m.displacement_normal(k).unwrap_or_else(Vec3::y)
        })
      };
      Rc::new(self.raster.verts.iter().map(|&k| of(k)).collect())
    }))
  }
}

/// Interpolated unit normal at a covered texel.
#[inline(always)]
fn unit(raster: &UvRaster, i: usize, normals: &[Vec3]) -> Vec3 {
  let n = raster.lerp(i, normals);
  n.try_normalize(1e-12).unwrap_or(n)
}

/// Three planes of a per-vertex vector field over the atlas; background texels are 0.
fn field_planes(
  raster: &UvRaster,
  vals: &[Vec3],
  normalize: bool,
) -> Result<Vec<Vec<f32>>, ErrorStack> {
  let mut planes = crate::tex_mem::try_planes(3, raster.w * raster.h)?;
  for i in (0..raster.w * raster.h).filter(|&i| raster.covered(i)) {
    let v = if normalize {
      unit(raster, i, vals)
    } else {
      raster.lerp(i, vals)
    };
    for c in 0..3 {
      planes[c][i] = v[c];
    }
  }
  Ok(planes)
}

/// Local- or world-space view of a mesh's geometry.
struct Frame {
  xf: Matrix4<f32>,
  normal_mat: Matrix3<f32>,
  world: bool,
}

impl Frame {
  fn new(mesh: &MeshHandle, world: bool) -> Self {
    let xf = mesh.transform;
    let linear = xf.fixed_view::<3, 3>(0, 0).into_owned();
    Self {
      xf,
      normal_mat: linear
        .try_inverse()
        .map(|inv| inv.transpose())
        .unwrap_or(linear),
      world,
    }
  }

  fn pos(&self, p: Vec3) -> Vec3 {
    if self.world {
      (self.xf * p.push(1.)).xyz()
    } else {
      p
    }
  }

  fn normal(&self, n: Vec3) -> Vec3 {
    if self.world {
      self.normal_mat * n
    } else {
      n
    }
  }

  fn length_scale(&self) -> f32 {
    if self.world {
      self.xf.fixed_view::<3, 3>(0, 0).determinant().abs().cbrt()
    } else {
      1.
    }
  }
}

/// Bilinear taps on a texture's mip chain. Reads planes directly (repeat or clamp wrap) since
/// bakes take millions of taps.
struct MipSampler<'a> {
  tex: &'a TextureHandle,
  base: (ArrayVec<&'a [f32], 4>, isize, isize, isize),
  mips: Rc<Vec<MipLevel>>,
  repeat: bool,
}

impl<'a> MipSampler<'a> {
  fn new(tex: &'a TextureHandle, wrap: TextureWrap) -> Self {
    Self {
      tex,
      base: tex.gather_parts(),
      mips: get_or_build_mips(tex),
      repeat: !matches!(wrap, TextureWrap::Clamp),
    }
  }

  #[inline(always)]
  fn level(&self, l: usize, u: f32, v: f32, out: &mut [f32; 4]) {
    let ml = (l > 0).then(|| &self.mips[l.min(self.mips.len()) - 1]);
    let (w, h, origin, xp, yp) = match ml {
      Some(ml) => (ml.width, ml.height, 0, 1, ml.width as isize),
      None => (
        self.tex.width,
        self.tex.height,
        self.base.1,
        self.base.2,
        self.base.3,
      ),
    };
    let (wi, hi) = (w as isize, h as isize);
    // Repeat folds the coordinate into one period up front, so the taps wrap with a compare
    // instead of a division.
    let (u, v) = if self.repeat {
      (u - u.floor(), v - v.floor())
    } else {
      (u, v)
    };
    let (x, y) = (u * w as f32 - 0.5, v * h as f32 - 0.5);
    let (x0f, y0f) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0f, y - y0f);
    let taps = |c: isize, n: isize| {
      if self.repeat {
        (
          if c < 0 { n - 1 } else { c.min(n - 1) },
          if c + 1 >= n { 0 } else { c + 1 },
        )
      } else {
        (c.clamp(0, n - 1), (c + 1).clamp(0, n - 1))
      }
    };
    let ((x0, x1), (y0, y1)) = (taps(x0f as isize, wi), taps(y0f as isize, hi));
    let at = |x: isize, y: isize| (origin + y * yp + x * xp) as usize;
    let (i00, i10, i01, i11) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
    let (w00, w10, w01, w11) = (
      (1. - fx) * (1. - fy),
      fx * (1. - fy),
      (1. - fx) * fy,
      fx * fy,
    );
    for (c, o) in out.iter_mut().enumerate().take(self.tex.channels) {
      let p: &[f32] = match ml {
        Some(ml) => &ml.planes[c],
        None => self.base.0[c],
      };
      *o = p[i00] * w00 + p[i10] * w10 + p[i01] * w01 + p[i11] * w11;
    }
  }

  /// Nearest mip level: the footprint is constant across a face, so there is no per-texel
  /// level transition to hide and the second tap of a trilinear blend buys nothing.
  fn level_for(&self, lod: f32) -> usize {
    lod.round().clamp(0., self.mips.len() as f32) as usize
  }

  fn sample(&self, u: f32, v: f32, lod: f32, out: &mut [f32; 4]) {
    self.level(self.level_for(lod), u, v, out);
  }
}

pub(crate) fn bake_texture_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (w, h) = dims(
    "bake_texture",
    arg_refs[0].resolve(args, kwargs),
    arg_refs[1].resolve(args, kwargs),
  )?;
  let cb = arg_refs[2].resolve(args, kwargs).as_callable().unwrap();
  let mesh = arg_refs[3].resolve(args, kwargs).as_mesh().unwrap();
  let dilate = arg_refs[4].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  let transformed = arg_refs[5].resolve(args, kwargs).as_bool().unwrap();

  // Which rasterized inputs the callback takes: `|pos|`, `|pos, normal|`, or either followed by
  // the texel's own `uv` / `x_ix` / `y_ix` (which `texture_zip` supplies after the inputs).
  let mut n_inputs = 2;
  if let Callable::Closure(c) = &**cb {
    if c.params.is_empty() {
      return Err(ErrorStack::new(
        "`bake_texture` callback needs at least a `pos` parameter: `|pos, normal, uv| ...`",
      ));
    }
    if c
      .params
      .iter()
      .any(|p| matches!(p.ident, DestructurePattern::Map(_)))
    {
      return Err(ErrorStack::new(
        "`bake_texture` callbacks can't take an attribute bag; rasterize attributes with \
         `bake_attr(w, h, name, mesh)` and combine them with `texture_zip`",
      ));
    }
    let texel_param = |i: usize| match c.params.get(i).map(|p| &p.ident) {
      Some(DestructurePattern::Ident(sym, _)) => {
        ctx.with_resolved_sym(*sym, |s| matches!(s, "uv" | "x_ix" | "y_ix"))
      }
      _ => false,
    };
    if c.params.len() == 1 || texel_param(1) {
      n_inputs = 1;
    }
  }

  let atlas = atlas_for(ctx, mesh, w, h, dilate)?;
  let raster = &atlas.raster;
  let fr = Frame::new(mesh, transformed);
  let mut inputs = vec![atlas_texture(
    w,
    h,
    field_planes(raster, &atlas.positions(mesh, &fr), false)?,
  )];
  if n_inputs == 2 {
    inputs.push(atlas_texture(
      w,
      h,
      field_planes(raster, &atlas.normals(mesh, &fr), true)?,
    ));
  }
  let refs: Vec<&TextureHandle> = inputs.iter().collect();
  let baked = zip_textures(ctx, cb, &refs)
    .map_err(|err| err.wrap("error calling `bake_texture` callback"))?;
  // The callback ran over background texels too (fed zeros); clear them so coverage is readable
  // off the result and stats aren't polluted.
  let Value::Texture(baked) = baked else {
    unreachable!()
  };
  let mut out = Rc::try_unwrap(baked).unwrap_or_else(|rc| (*rc).clone());
  let mut planes = out.as_planes();
  for plane in planes.iter_mut() {
    let plane = Rc::make_mut(plane);
    for i in 0..plane.len() {
      if !raster.covered(i) {
        plane[i] = 0.;
      }
    }
  }
  out.storage = TexStorage::planes(planes);
  out.mips = Default::default();
  Ok(Value::Texture(Rc::new(out)))
}

pub(crate) fn bake_attr_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (w, h) = dims(
    "bake_attr",
    arg_refs[0].resolve(args, kwargs),
    arg_refs[1].resolve(args, kwargs),
  )?;
  let name = arg_refs[2].resolve(args, kwargs).as_str().unwrap();
  let mesh = arg_refs[3].resolve(args, kwargs).as_mesh().unwrap();
  let dilate = arg_refs[4].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  let ch = mesh.mesh.vertex_channels.get(name).ok_or_else(|| {
    ErrorStack::new(format!(
      "Mesh has no attribute `{name}`; available: {}",
      available(&mesh.mesh)
    ))
  })?;
  let atlas = atlas_for(ctx, mesh, w, h, dilate)?;
  let mut planes = atlas
    .raster
    .interpolate(ch.store.arity(), |k| ch.get(k).unwrap_or_default())?;
  if ch.interp == Interp::LerpNormalize {
    renormalize(&mut planes);
  }
  Ok(Value::Texture(Rc::new(atlas_texture(w, h, planes))))
}

pub(crate) fn bake_sdf_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (w, h) = dims(
    "bake_sdf",
    arg_refs[0].resolve(args, kwargs),
    arg_refs[1].resolve(args, kwargs),
  )?;
  let mask = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();
  let mesh = arg_refs[3].resolve(args, kwargs).as_mesh().unwrap();
  let max_dist = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
  if max_dist <= 0. {
    return Err(ErrorStack::new("`bake_sdf` `max_dist` must be > 0"));
  }
  let signed = arg_refs[5].resolve(args, kwargs).as_bool().unwrap();
  let dilate = arg_refs[6].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  let transformed = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();

  let bvh = mask.get_or_create_bvh();
  if bvh.closest_point(&Vec3::zeros()).is_none() {
    return Err(ErrorStack::new("`bake_sdf` mask mesh has no faces"));
  }
  let atlas = atlas_for(ctx, mesh, w, h, dilate)?;
  let raster = &atlas.raster;
  let pos = atlas.positions(mesh, &Frame::new(mesh, transformed));
  let at = |i: usize| raster.lerp(i, &pos);
  let d_at = |p: Vec3, reach: f32| bvh.signed_distance(&p, reach, signed).unwrap();
  let mut out = crate::tex_mem::try_vec(w * h, 0f32)?;
  // Distance fields are 1-Lipschitz: a block whose centre is farther than its half-diagonal
  // plus `max_dist` from the surface is uniformly saturated.
  const B: usize = 8;
  for by in (0..h).step_by(B) {
    for bx in (0..w).step_by(B) {
      let texels =
        (by..(by + B).min(h)).flat_map(|y| (bx..(bx + B).min(w)).map(move |x| y * w + x));
      let (mut lo, mut hi) = (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY));
      for i in texels.clone().filter(|&i| raster.covered(i)) {
        lo = lo.inf(&at(i));
        hi = hi.sup(&at(i));
      }
      if lo.x == f32::INFINITY {
        continue;
      }
      let reach = max_dist + (hi - lo).norm() * 0.5;
      let dc = d_at((lo + hi) * 0.5, reach);
      let fill = (dc.abs() >= reach).then(|| dc.signum() * max_dist);
      for i in texels.filter(|&i| raster.covered(i)) {
        out[i] = fill.unwrap_or_else(|| d_at(at(i), max_dist));
      }
    }
  }
  Ok(Value::Texture(Rc::new(atlas_texture(w, h, vec![out]))))
}

pub(crate) fn bake_triplanar_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (w, h) = dims(
    "bake_triplanar",
    arg_refs[0].resolve(args, kwargs),
    arg_refs[1].resolve(args, kwargs),
  )?;
  let tex = arg_refs[2].resolve(args, kwargs).as_texture().unwrap();
  let mesh = arg_refs[3].resolve(args, kwargs).as_mesh().unwrap();
  let scale = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
  let sharpness = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
  let dilate = arg_refs[6].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  let transformed = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();

  let atlas = atlas_for(ctx, mesh, w, h, dilate)?;
  let raster = &atlas.raster;
  let fr = Frame::new(mesh, transformed);
  let pos = atlas.positions(mesh, &fr);
  let nrm = atlas.normals(mesh, &fr);
  let sampler = MipSampler::new(tex, tex.wrap);
  let pw = |x: f32| {
    if sharpness == 4. {
      (x * x) * (x * x)
    } else {
      x.powf(sharpness)
    }
  };
  let tex_px = tex.width.max(tex.height) as f32 * scale * fr.length_scale();
  let face_level: Vec<u8> = raster
    .face_texel
    .iter()
    .map(|t| sampler.level_for((t * tex_px).max(1.).log2()) as u8)
    .collect();
  let ch = tex.channels;
  let n = w * h;
  let mut planes = crate::tex_mem::try_planes(ch, n)?;
  for i in 0..n {
    if !raster.covered(i) {
      continue;
    }
    let p = raster.lerp(i, &pos);
    let nv = unit(raster, i, &nrm);
    let mut wgt = [pw(nv.x.abs()), pw(nv.y.abs()), pw(nv.z.abs())];
    let mut sum: f32 = wgt.iter().sum();
    if sum <= 1e-12 {
      wgt = [0., 0., 1.];
      sum = 1.;
    }
    let level = face_level[raster.face[i] as usize] as usize;
    let uvs = [(p.y, p.z), (p.z, p.x), (p.x, p.y)];
    let mut acc = [0f32; 4];
    for (ax, (u, v)) in uvs.iter().enumerate() {
      if wgt[ax] < 1e-3 * sum {
        continue;
      }
      let mut px = [0f32; 4];
      sampler.level(level, u * scale, v * scale, &mut px);
      for c in 0..ch {
        acc[c] += px[c] * wgt[ax] / sum;
      }
    }
    for c in 0..ch {
      planes[c][i] = acc[c];
    }
  }
  Ok(Value::Texture(Rc::new(atlas_texture(w, h, planes))))
}

pub(crate) fn bake_decal_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (w, h) = dims(
    "bake_decal",
    arg_refs[0].resolve(args, kwargs),
    arg_refs[1].resolve(args, kwargs),
  )?;
  let stamp = arg_refs[2].resolve(args, kwargs).as_texture().unwrap();
  let mesh = arg_refs[3].resolve(args, kwargs).as_mesh().unwrap();
  let xf: Matrix4<f32> = match arg_refs[4].resolve(args, kwargs) {
    Value::Nil => Matrix4::identity(),
    v => *v.as_mat4().unwrap(),
  };
  let depth = match arg_refs[5].resolve(args, kwargs) {
    Value::Nil => f32::INFINITY,
    v => v.as_float().unwrap(),
  };
  let two_sided = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
  let dilate = arg_refs[7].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  if stamp.channels == 2 {
    return Err(ErrorStack::new(
      "`bake_decal` stamps must have 1, 3, or 4 channels (a 4th channel is alpha)",
    ));
  }
  let inv = xf
    .try_inverse()
    .ok_or_else(|| ErrorStack::new("`bake_decal` transform is singular"))?;
  let z_world = (xf * Vector4::new(0., 0., 1., 0.)).xyz().normalize();
  let [sx, sy, sz] = [0, 1, 2].map(|c| xf.fixed_view::<3, 1>(0, c).norm());

  let atlas = atlas_for(ctx, mesh, w, h, dilate)?;
  let raster = &atlas.raster;
  let fr = Frame::new(mesh, true);
  let pos = atlas.positions(mesh, &fr);
  let nrm = atlas.normals(mesh, &fr);
  let sampler = MipSampler::new(stamp, TextureWrap::Clamp);
  let (sch, out_ch) = (
    stamp.channels,
    if stamp.channels == 4 {
      4
    } else {
      stamp.channels + 1
    },
  );
  let texel_scale = fr.length_scale();
  let n = w * h;
  let mut planes = crate::tex_mem::try_planes(out_ch, n)?;
  for i in 0..n {
    if !raster.covered(i) {
      continue;
    }
    let p = raster.lerp(i, &pos);
    let nv = unit(raster, i, &nrm);
    let q = (inv * p.push(1.)).xyz();
    if q.x.abs() > 0.5
      || q.y.abs() > 0.5
      || q.z.abs() * sz > depth
      || !(two_sided || nv.dot(&z_world) > 0.)
    {
      continue;
    }
    let t = raster.texel_size(i) * texel_scale;
    let lod = (t * stamp.width as f32 / sx)
      .max(t * stamp.height as f32 / sy)
      .max(1.)
      .log2();
    let mut px = [0f32; 4];
    sampler.sample(q.x + 0.5, q.y + 0.5, lod, &mut px);
    for c in 0..sch {
      planes[c][i] = px[c];
    }
    if sch != 4 {
      planes[sch][i] = 1.;
    }
  }
  Ok(Value::Texture(Rc::new(atlas_texture(w, h, planes))))
}

pub(crate) fn bake_ao_texture_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (w, h) = dims(
    "bake_ao_texture",
    arg_refs[0].resolve(args, kwargs),
    arg_refs[1].resolve(args, kwargs),
  )?;
  let mesh = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();
  let samples = arg_refs[3].resolve(args, kwargs).as_int().unwrap().max(1) as usize;
  let max_dist = match arg_refs[4].resolve(args, kwargs) {
    Value::Nil => f32::MAX,
    v => v.as_float().unwrap(),
  };
  let occluders: Vec<Rc<MeshHandle>> = match arg_refs[5].resolve(args, kwargs) {
    Value::Nil => Vec::new(),
    Value::Mesh(m) => vec![Rc::clone(m)],
    Value::Sequence(seq) => seq
      .consume(ctx)
      .map(|v| match v? {
        Value::Mesh(m) => Ok(m),
        other => Err(ErrorStack::new(format!(
          "`bake_ao_texture` occluders must be meshes; got {other:?}"
        ))),
      })
      .collect::<Result<_, _>>()?,
    _ => unreachable!(),
  };
  let bias = match arg_refs[6].resolve(args, kwargs) {
    Value::Nil => {
      let aabb = mesh.get_or_compute_aabb();
      ((aabb.maxs - aabb.mins).norm() * 1e-4).max(1e-6)
    }
    v => v.as_float().unwrap(),
  };
  let dilate = arg_refs[7].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  let intensity = ao_intensity("bake_ao_texture", arg_refs[8].resolve(args, kwargs))?;

  let atlas = atlas_for(ctx, mesh, w, h, dilate)?;
  let raster = &atlas.raster;
  let fr = Frame::new(mesh, true);
  let pos = atlas.positions(mesh, &fr);
  let nrm = atlas.normals(mesh, &fr);
  let params = AoParams {
    samples,
    max_dist,
    bias,
    intensity,
  };
  let sampler = Sampler::new(mesh, &occluders, &params);
  let n = w * h;
  // R2 sequence over texel coordinates: low-discrepancy rotations turn the estimator's white
  // noise into high-frequency structure that `blur_atlas` removes cleanly.
  let rot = |i: usize| {
    ((0.754_877_7 * (i % w) as f32 + 0.569_840_3 * (i / w) as f32).fract()) * std::f32::consts::TAU
  };
  let at = |i: usize| (raster.lerp(i, &pos), unit(raster, i, &nrm));
  const OPEN: u8 = u8::MAX;
  // Pass 1: a strided probe subset everywhere the tangent plane isn't provably open. Pass 2:
  // the remaining rays only where the probe's 3×3 neighbourhood is neither all-open nor
  // all-buried, so the full budget lands on partially occluded texels and every ray cast
  // counts toward the estimate (no early-out bias).
  let k = (samples / 4)
    .clamp(1, samples)
    .max(8.min(samples))
    .min(OPEN as usize - 1);
  let probe: Vec<usize> = (0..k).map(|j| j * samples / k).collect();
  let rest: Vec<usize> = (0..samples).filter(|i| !probe.contains(i)).collect();
  let mut hits = vec![OPEN; n];
  for i in 0..n {
    if !raster.covered(i) {
      continue;
    }
    let (p, nv) = at(i);
    if !sampler.open_above(p, nv) {
      hits[i] = sampler.escaped(rot(i), p, nv, &probe) as u8;
    }
  }
  let (wi, hi) = (w as isize, h as isize);
  let mut out = crate::tex_mem::try_vec(n, 0f32)?;
  for i in 0..n {
    if !raster.covered(i) {
      continue;
    }
    if hits[i] == OPEN {
      out[i] = 1.;
      continue;
    }
    let (x, y) = ((i % w) as isize, (i / w) as isize);
    let mut lo = k as u8;
    let mut up = 0u8;
    for dy in -1..=1 {
      for dx in -1..=1 {
        let (nx, ny) = (x + dx, y + dy);
        if nx < 0 || ny < 0 || nx >= wi || ny >= hi {
          continue;
        }
        let j = (ny * wi + nx) as usize;
        if !raster.covered(j) {
          continue;
        }
        let e = if hits[j] == OPEN { k as u8 } else { hits[j] };
        lo = lo.min(e);
        up = up.max(e);
      }
    }
    let settled = (lo == k as u8 && up == k as u8) || (lo == 0 && up == 0);
    out[i] = sampler.shape(if settled || rest.is_empty() {
      hits[i] as f32 / k as f32
    } else {
      let (p, nv) = at(i);
      (hits[i] as u32 + sampler.escaped(rot(i), p, nv, &rest)) as f32 / samples as f32
    });
  }
  Ok(Value::Texture(Rc::new(atlas_texture(w, h, vec![out]))))
}

pub(crate) fn blur_atlas_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let radius = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let tex = arg_refs[2].resolve(args, kwargs).as_texture().unwrap();
  let dilate = arg_refs[3].resolve(args, kwargs).as_int().unwrap().max(0) as usize;
  if radius <= 0. {
    return Ok(Value::Texture(Rc::clone(tex)));
  }
  let (w, h) = (tex.width, tex.height);
  let atlas = atlas_for(ctx, mesh, w, h, dilate)?;
  let raster = &atlas.raster;
  let fr = Frame::new(mesh, true);
  let pos = atlas.positions(mesh, &fr);
  let nrm = atlas.normals(mesh, &fr);
  let src = tex.as_planes();
  let r = radius.ceil() as isize;
  let mut planes = crate::tex_mem::try_planes(tex.channels, w * h)?;
  // Every texel is read (2r+1)² times, so this one bake interpolates the geometry up front.
  let n = w * h;
  let (mut pts, mut nrms) = (
    crate::tex_mem::try_vec(n, Vec3::zeros())?,
    crate::tex_mem::try_vec(n, Vec3::zeros())?,
  );
  for i in (0..n).filter(|&i| raster.covered(i)) {
    (pts[i], nrms[i]) = (raster.lerp(i, &pos), unit(raster, i, &nrm));
  }
  let at = |i: usize| pts[i];
  let nv = |i: usize| nrms[i];
  for y in 0..h as isize {
    for x in 0..w as isize {
      let i = (y * w as isize + x) as usize;
      if !raster.covered(i) {
        continue;
      }
      let (pi, ni) = (at(i), nv(i));
      // Gaussian over WORLD distance (σ = radius texels of this face's texel size), gated by
      // normal agreement: neighbours across a UV seam are far away in 3D, neighbours across a
      // crease face the other way, so neither leaks in.
      let sigma = radius * raster.texel_size(i) * fr.length_scale();
      let inv2s2 = 1. / (2. * sigma * sigma).max(1e-12);
      let (mut acc, mut wsum) = ([0f32; 4], 0f32);
      for dy in -r..=r {
        for dx in -r..=r {
          let (nx, ny) = (x + dx, y + dy);
          if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
            continue;
          }
          let j = (ny * w as isize + nx) as usize;
          if !raster.covered(j) {
            continue;
          }
          let d2 = (at(j) - pi).norm_squared();
          let align = ni.dot(&nv(j)).max(0.);
          let e = d2 * inv2s2;
          if e > BLUR_CUTOFF || align <= 0. {
            continue;
          }
          let wt = fm::exp(-e) * align * align * align * align;
          for (c, plane) in src.iter().enumerate() {
            acc[c] += plane[j] * wt;
          }
          wsum += wt;
        }
      }
      for (c, plane) in planes.iter_mut().enumerate() {
        plane[i] = if wsum > 0. { acc[c] / wsum } else { src[c][i] };
      }
    }
  }
  Ok(Value::Texture(Rc::new(TextureHandle {
    storage: TexStorage::from_plane_vecs(planes),
    mips: Default::default(),
    ..(**tex).clone()
  })))
}
