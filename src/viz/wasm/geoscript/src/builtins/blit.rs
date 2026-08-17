//! Blitting textures into other textures: the shared core behind `blit` and `scatter`.
//!
//! A stamp's `transform` maps its centered local frame ([-0.5, 0.5]², origin = center)
//! into the base texture's UV space. Reads use decal semantics (the stamp's own wrap mode
//! is ignored; outside its bounds contributes nothing); writes follow the BASE's wrap mode
//! (`Repeat` wraps the footprint around edges so seamless tiling is preserved; `Clamp` and
//! `Mirror` clip).

use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  ArgRef, ErrorStack, EvalCtx, MipLevel, Sym, TextureHandle, TextureWrap, Value, EMPTY_KWARGS,
};

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BlendMode {
  Over,
  Add,
  Sub,
  Mul,
  Max,
  Min,
}

impl BlendMode {
  pub(crate) fn from_name(s: &str) -> Result<Self, ErrorStack> {
    match s {
      "over" => Ok(Self::Over),
      "add" => Ok(Self::Add),
      "sub" => Ok(Self::Sub),
      "mul" => Ok(Self::Mul),
      "max" => Ok(Self::Max),
      "min" => Ok(Self::Min),
      _ => Err(ErrorStack::new(format!(
        "Invalid blend mode: \"{s}\"; expected one of \"over\", \"add\", \"sub\", \"mul\", \
         \"max\", \"min\""
      ))),
    }
  }

  fn apply(self, base: f32, stamp: f32) -> f32 {
    match self {
      Self::Over => stamp,
      Self::Add => base + stamp,
      Self::Sub => base - stamp,
      Self::Mul => base * stamp,
      Self::Max => base.max(stamp),
      Self::Min => base.min(stamp),
    }
  }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BlitFilter {
  Nearest,
  /// Bilinear magnification + trilinear (mip) minification.
  Bilinear,
}

impl BlitFilter {
  pub(crate) fn from_name(s: &str) -> Result<Self, ErrorStack> {
    match s {
      "nearest" => Ok(Self::Nearest),
      "bilinear" => Ok(Self::Bilinear),
      _ => Err(ErrorStack::new(format!(
        "Invalid blit filter: \"{s}\"; expected \"nearest\" or \"bilinear\""
      ))),
    }
  }
}

/// The 2D affine part of a placement mat4: p' = A·p + t.
#[derive(Clone, Copy)]
struct Affine2 {
  a00: f32,
  a01: f32,
  a10: f32,
  a11: f32,
  tx: f32,
  ty: f32,
}

impl Affine2 {
  fn from_mat4(m: &crate::Mat4) -> Self {
    Self {
      a00: m[(0, 0)],
      a01: m[(0, 1)],
      a10: m[(1, 0)],
      a11: m[(1, 1)],
      tx: m[(0, 3)],
      ty: m[(1, 3)],
    }
  }

  fn inverse(&self) -> Option<Self> {
    let det = self.a00 * self.a11 - self.a01 * self.a10;
    if det.abs() < 1e-12 {
      return None;
    }
    let inv_det = 1. / det;
    let (a00, a01, a10, a11) = (
      self.a11 * inv_det,
      -self.a01 * inv_det,
      -self.a10 * inv_det,
      self.a00 * inv_det,
    );
    Some(Self {
      a00,
      a01,
      a10,
      a11,
      tx: -(a00 * self.tx + a01 * self.ty),
      ty: -(a10 * self.tx + a11 * self.ty),
    })
  }

  fn apply(&self, x: f32, y: f32) -> (f32, f32) {
    (
      self.a00 * x + self.a01 * y + self.tx,
      self.a10 * x + self.a11 * y + self.ty,
    )
  }
}

/// Stamp channels → (value channel count, whether the last channel is alpha), validated
/// against the base's channel count.
fn channel_layout(stamp_ch: usize, base_ch: usize) -> Result<(usize, bool), ErrorStack> {
  match (stamp_ch, base_ch) {
    (4, 3) | (4, 4) => Ok((3, true)),
    (2, 1) => Ok((1, true)),
    (s, b) if s == b => Ok((s, false)),
    (1, 3) | (1, 4) => Ok((1, false)),
    (3, 4) => Ok((3, false)),
    (s, b) => Err(ErrorStack::new(format!(
      "Cannot blit a {s}-channel texture onto a {b}-channel texture; supported: same channel \
       count, stamp with one extra (alpha) channel, or 1-channel (gray) onto 3/4 channels"
    ))),
  }
}

/// Mip levels 1.. via successive box-halving. 4-channel textures are averaged
/// premultiplied so RGB doesn't bleed through transparent texels.
fn get_or_build_mips(tex: &TextureHandle) -> Rc<Vec<MipLevel>> {
  let src_ptr = Rc::as_ptr(&tex.pixels) as usize;
  if let Some(chain) = tex.mips.0.borrow().as_ref() {
    if chain.src == src_ptr {
      return Rc::clone(&chain.levels);
    }
  }

  let ch = tex.channels;
  let premult = ch == 4;
  let mut levels: Vec<MipLevel> = Vec::new();
  let (mut pw, mut ph) = (tex.width, tex.height);
  while pw > 1 || ph > 1 {
    let (nw, nh) = ((pw / 2).max(1), (ph / 2).max(1));
    let src: &[f32] = match levels.last() {
      Some(l) => &l.pixels,
      None => &tex.pixels,
    };
    let mut px = vec![0f32; nw * nh * ch];
    for y in 0..nh {
      let (y0, y1) = ((y * 2).min(ph - 1), (y * 2 + 1).min(ph - 1));
      for x in 0..nw {
        let (x0, x1) = ((x * 2).min(pw - 1), (x * 2 + 1).min(pw - 1));
        let taps = [
          (y0 * pw + x0) * ch,
          (y0 * pw + x1) * ch,
          (y1 * pw + x0) * ch,
          (y1 * pw + x1) * ch,
        ];
        let out = &mut px[(y * nw + x) * ch..(y * nw + x) * ch + ch];
        if premult {
          let mut acc = [0f32; 4];
          for t in taps {
            let a = src[t + 3];
            for c in 0..3 {
              acc[c] += src[t + c] * a;
            }
            acc[3] += a;
          }
          let avg_a = acc[3] * 0.25;
          if avg_a > 1e-8 {
            for c in 0..3 {
              out[c] = acc[c] * 0.25 / avg_a;
            }
          }
          out[3] = avg_a;
        } else {
          for c in 0..ch {
            out[c] = (src[taps[0] + c] + src[taps[1] + c] + src[taps[2] + c] + src[taps[3] + c])
              * 0.25;
          }
        }
      }
    }
    levels.push(MipLevel {
      pixels: px,
      width: nw,
      height: nh,
    });
    (pw, ph) = (nw, nh);
  }

  let levels = Rc::new(levels);
  *tex.mips.0.borrow_mut() = Some(crate::MipChain {
    src: src_ptr,
    levels: Rc::clone(&levels),
  });
  levels
}

struct LevelView<'a> {
  px: &'a [f32],
  w: usize,
  h: usize,
}

/// Single-level decal sample at stamp-local coords. Accumulates premultiplied so
/// out-of-bounds (transparent) taps don't bleed; returns straight values + effective alpha
/// (stamp alpha × edge coverage).
fn sample_level(
  view: &LevelView,
  ch: usize,
  val_ch: usize,
  has_alpha: bool,
  lx: f32,
  ly: f32,
  filter: BlitFilter,
) -> ([f32; 3], f32) {
  let sx = (lx + 0.5) * view.w as f32 - 0.5;
  let sy = (ly + 0.5) * view.h as f32 - 0.5;

  let tap = |tx: i64, ty: i64| -> Option<([f32; 3], f32)> {
    if tx < 0 || ty < 0 || tx >= view.w as i64 || ty >= view.h as i64 {
      return None;
    }
    let base = (ty as usize * view.w + tx as usize) * ch;
    let a = if has_alpha { view.px[base + ch - 1] } else { 1. };
    let mut vals = [0f32; 3];
    for (c, val) in vals.iter_mut().enumerate().take(val_ch) {
      *val = view.px[base + c];
    }
    Some((vals, a))
  };

  match filter {
    BlitFilter::Nearest => {
      let (tx, ty) = (
        (sx + 0.5).floor() as i64,
        (sy + 0.5).floor() as i64,
      );
      tap(tx, ty).unwrap_or(([0.; 3], 0.))
    }
    BlitFilter::Bilinear => {
      let (x0, y0) = (sx.floor(), sy.floor());
      let (fx, fy) = (sx - x0, sy - y0);
      let weights = [
        (x0 as i64, y0 as i64, (1. - fx) * (1. - fy)),
        (x0 as i64 + 1, y0 as i64, fx * (1. - fy)),
        (x0 as i64, y0 as i64 + 1, (1. - fx) * fy),
        (x0 as i64 + 1, y0 as i64 + 1, fx * fy),
      ];
      let mut acc = [0f32; 3];
      let mut acc_a = 0f32;
      for (tx, ty, w) in weights {
        if let Some((vals, a)) = tap(tx, ty) {
          let wa = w * a;
          for c in 0..val_ch {
            acc[c] += vals[c] * wa;
          }
          acc_a += wa;
        }
      }
      if acc_a > 1e-8 {
        for v in acc.iter_mut().take(val_ch) {
          *v /= acc_a;
        }
      }
      (acc, acc_a)
    }
  }
}

/// Blits `stamp` (placed by its transform) into a mutable pixel buffer described by the
/// base's dimensions/wrap. A degenerate (zero-scale) placement is a no-op.
pub(crate) fn blit_into(
  base_px: &mut [f32],
  bw: usize,
  bh: usize,
  bch: usize,
  bwrap: TextureWrap,
  stamp: &TextureHandle,
  blend: BlendMode,
  filter: BlitFilter,
) -> Result<(), ErrorStack> {
  let (val_ch, has_alpha) = channel_layout(stamp.channels, bch)?;
  let fwd = Affine2::from_mat4(&stamp.transform);
  let Some(inv) = fwd.inverse() else {
    return Ok(());
  };

  // Minification level from the (constant, since affine) local→texel Jacobian.
  let (sw, sh) = (stamp.width, stamp.height);
  let mips = if filter == BlitFilter::Bilinear {
    let jx = (
      inv.a00 / bw as f32 * sw as f32,
      inv.a10 / bw as f32 * sh as f32,
    );
    let jy = (
      inv.a01 / bh as f32 * sw as f32,
      inv.a11 / bh as f32 * sh as f32,
    );
    let rho = (jx.0 * jx.0 + jx.1 * jx.1)
      .max(jy.0 * jy.0 + jy.1 * jy.1)
      .sqrt();
    if rho > 1.001 {
      Some((rho.log2(), get_or_build_mips(stamp)))
    } else {
      None
    }
  } else {
    None
  };

  // (level view, secondary view for trilinear, lerp factor)
  let base_view = LevelView {
    px: &stamp.pixels,
    w: sw,
    h: sh,
  };
  let (view0, view1, level_frac) = match &mips {
    None => (base_view, None, 0.),
    Some((level_f, levels)) => {
      let l0 = (level_f.floor() as usize).min(levels.len());
      let l1 = (l0 + 1).min(levels.len());
      let frac = if l0 == l1 { 0. } else { level_f - l0 as f32 };
      let view_of = |l: usize| -> LevelView {
        if l == 0 {
          LevelView {
            px: &stamp.pixels,
            w: sw,
            h: sh,
          }
        } else {
          let ml = &levels[l - 1];
          LevelView {
            px: &ml.pixels,
            w: ml.width,
            h: ml.height,
          }
        }
      };
      (
        view_of(l0),
        if l1 != l0 { Some(view_of(l1)) } else { None },
        frac,
      )
    }
  };

  // Filter reach in local units: one texel of the coarsest sampled level.
  let eps = match filter {
    BlitFilter::Nearest => 0.,
    BlitFilter::Bilinear => {
      let coarse = view1.as_ref().unwrap_or(&view0);
      1. / (coarse.w.min(coarse.h) as f32)
    }
  };

  // Footprint AABB in base pixel space from the transformed local corners, expanded by the
  // filter reach (so a minified stamp's bilinear skirt isn't clipped) plus a pixel of slop.
  let e = 0.5 + eps;
  let corners = [
    fwd.apply(-e, -e),
    fwd.apply(e, -e),
    fwd.apply(-e, e),
    fwd.apply(e, e),
  ];
  let (mut min_u, mut max_u, mut min_v, mut max_v) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
  for (u, v) in corners {
    (min_u, max_u) = (min_u.min(u), max_u.max(u));
    (min_v, max_v) = (min_v.min(v), max_v.max(v));
  }
  let mut x0 = (min_u * bw as f32 - 0.5).floor() as i64 - 1;
  let mut x1 = (max_u * bw as f32 - 0.5).ceil() as i64 + 1;
  let mut y0 = (min_v * bh as f32 - 0.5).floor() as i64 - 1;
  let mut y1 = (max_v * bh as f32 - 0.5).ceil() as i64 + 1;
  if bwrap != TextureWrap::Repeat {
    x0 = x0.max(0);
    x1 = x1.min(bw as i64 - 1);
    y0 = y0.max(0);
    y1 = y1.min(bh as i64 - 1);
  }
  if x0 > x1 || y0 > y1 {
    return Ok(());
  }
  // Bounds runaway placements (huge scales / saturated casts): on a Repeat base the
  // footprint is deliberately NOT clamped to one period (self-overlap is real coverage),
  // so cap total iterated area instead of hanging the worker.
  const MAX_FOOTPRINT_PX: i64 = 8192 * 8192;
  if (x1 - x0 + 1).saturating_mul(y1 - y0 + 1) > MAX_FOOTPRINT_PX {
    return Err(ErrorStack::new(format!(
      "blit footprint too large: {}x{} pixels (max {MAX_FOOTPRINT_PX} total); check the \
       stamp's placement scale",
      x1 - x0 + 1,
      y1 - y0 + 1
    )));
  }

  for y in y0..=y1 {
    let v = (y as f32 + 0.5) / bh as f32;
    let wy = (y.rem_euclid(bh as i64)) as usize;
    for x in x0..=x1 {
      let u = (x as f32 + 0.5) / bw as f32;
      let (lx, ly) = inv.apply(u, v);
      if lx < -0.5 - eps || lx > 0.5 + eps || ly < -0.5 - eps || ly > 0.5 + eps {
        continue;
      }

      let (mut vals, mut sa) = sample_level(&view0, stamp.channels, val_ch, has_alpha, lx, ly, filter);
      if let Some(v1) = &view1 {
        // Cross-level lerp runs premultiplied: straight-value blending would let a
        // low-alpha level's RGB bleed at alpha silhouettes.
        let (vals1, sa1) = sample_level(v1, stamp.channels, val_ch, has_alpha, lx, ly, filter);
        let sa_l = sa + (sa1 - sa) * level_frac;
        for c in 0..val_ch {
          let pm = vals[c] * sa + (vals1[c] * sa1 - vals[c] * sa) * level_frac;
          vals[c] = if sa_l > 1e-8 { pm / sa_l } else { 0. };
        }
        sa = sa_l;
      }
      if sa <= 0. {
        continue;
      }
      let sa = sa.min(1.);

      let wx = (x.rem_euclid(bw as i64)) as usize;
      let bpx = &mut base_px[(wy * bw + wx) * bch..(wy * bw + wx) * bch + bch];
      let bvc = if bch == 4 { 3 } else { bch };
      if bch == 4 && blend == BlendMode::Over {
        // Proper straight-alpha over for an RGBA base
        let ba = bpx[3];
        let out_a = sa + ba * (1. - sa);
        if out_a > 1e-8 {
          for c in 0..3 {
            let sv = vals[if val_ch == 1 { 0 } else { c }];
            bpx[c] = (sv * sa + bpx[c] * ba * (1. - sa)) / out_a;
          }
        }
        bpx[3] = out_a;
      } else {
        for c in 0..bvc {
          let sv = vals[if val_ch == 1 { 0 } else { c }];
          bpx[c] += (blend.apply(bpx[c], sv) - bpx[c]) * sa;
        }
        if bch == 4 {
          bpx[3] += sa * (1. - bpx[3]);
        }
      }
    }
  }

  Ok(())
}

fn resolve_blend_and_filter(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  blend_ix: usize,
) -> Result<(BlendMode, BlitFilter), ErrorStack> {
  let blend = BlendMode::from_name(arg_refs[blend_ix].resolve(args, kwargs).as_str().unwrap())?;
  let filter = BlitFilter::from_name(
    arg_refs[blend_ix + 1]
      .resolve(args, kwargs)
      .as_str()
      .unwrap(),
  )?;
  Ok((blend, filter))
}

pub(crate) fn blit_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let stamp = arg_refs[0].resolve(args, kwargs).as_texture().unwrap();
  let base = arg_refs[1].resolve(args, kwargs).as_texture().unwrap();
  let (blend, filter) = resolve_blend_and_filter(arg_refs, args, kwargs, 2)?;

  let mut px = (*base.pixels).clone();
  blit_into(
    &mut px,
    base.width,
    base.height,
    base.channels,
    base.wrap,
    stamp,
    blend,
    filter,
  )?;
  Ok(Value::Texture(Rc::new(TextureHandle {
    pixels: Rc::new(px),
    ..(**base).clone()
  })))
}

pub(crate) fn scatter_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (base_ix, blend_ix) = match def_ix {
    0 => (2, 3),
    1 => (1, 2),
    _ => unimplemented!(),
  };
  let base = arg_refs[base_ix]
    .resolve(args, kwargs)
    .as_texture()
    .unwrap();
  let (blend, filter) = resolve_blend_and_filter(arg_refs, args, kwargs, blend_ix)?;
  let (bw, bh, bch, bwrap) = (base.width, base.height, base.channels, base.wrap);
  let mut px = (*base.pixels).clone();

  let mut blit_stamp = |val: &Value, ix: usize| -> Result<(), ErrorStack> {
    let stamp = val.as_texture().ok_or_else(|| {
      ErrorStack::new(format!(
        "`scatter` expected a texture for instance {ix}, found: {val:?}"
      ))
    })?;
    blit_into(&mut px, bw, bh, bch, bwrap, stamp, blend, filter)
      .map_err(|err| err.wrap(format!("Error blitting `scatter` instance {ix}")))
  };

  match def_ix {
    0 => {
      let count = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if !(0..=1_000_000).contains(&count) {
        return Err(ErrorStack::new(format!(
          "Invalid `scatter` count: {count}; expected 0..=1000000"
        )));
      }
      let gen = arg_refs[1].resolve(args, kwargs).as_callable().unwrap();
      for ix in 0..count {
        let out = ctx
          .invoke_callable(gen, &[Value::Int(ix)], EMPTY_KWARGS)
          .map_err(|err| {
            err.wrap(format!(
              "Error produced by `stamps` generator callable in `scatter` for instance {ix}"
            ))
          })?;
        blit_stamp(&out, ix as usize)?;
      }
    }
    1 => {
      let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      for (ix, item) in seq.consume(ctx).enumerate() {
        blit_stamp(&item?, ix)?;
      }
    }
    _ => unimplemented!(),
  }

  Ok(Value::Texture(Rc::new(TextureHandle {
    pixels: Rc::new(px),
    ..(**base).clone()
  })))
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

  #[test]
  fn blit_placement_wrap_and_clamp() {
    let ctx = parse_and_eval_program(
      r#"
base = texture(4, 4, |uv| 0.)
base_clamp = texture(4, 4, |uv| 0., wrap="clamp")
stamp = texture(2, 2, |uv| 1.)
quarter = blit(stamp | scale(0.5) | trans_global(0.25, 0.25), base, filter="nearest")
wrapped = blit(stamp | scale(0.5), base, filter="nearest")
clamped = blit(stamp | scale(0.5), base_clamp, filter="nearest")
"#,
    )
    .unwrap();

    let quarter = get_tex(&ctx, "quarter");
    for y in 0..4 {
      for x in 0..4 {
        let expected = if x < 2 && y < 2 { 1. } else { 0. };
        assert_eq!(quarter.pixels[y * 4 + x], expected, "quarter ({x}, {y})");
      }
    }

    // Centered at UV (0,0) on a repeat-wrapped base: the footprint wraps to all 4 corners
    let wrapped = get_tex(&ctx, "wrapped");
    for y in 0..4 {
      for x in 0..4 {
        let expected = if (x == 0 || x == 3) && (y == 0 || y == 3) {
          1.
        } else {
          0.
        };
        assert_eq!(wrapped.pixels[y * 4 + x], expected, "wrapped ({x}, {y})");
      }
    }

    // Same placement on a clamp base: out-of-bounds writes clip
    let clamped = get_tex(&ctx, "clamped");
    for y in 0..4 {
      for x in 0..4 {
        let expected = if x == 0 && y == 0 { 1. } else { 0. };
        assert_eq!(clamped.pixels[y * 4 + x], expected, "clamped ({x}, {y})");
      }
    }
  }

  #[test]
  fn blit_alpha_over_and_height_blends() {
    let ctx = parse_and_eval_program(
      r#"
base3 = texture(2, 2, |uv| v3(0.2, 0.2, 0.2))
cover = |t: texture|: texture t | trans_global(0.5, 0.5)
over = blit(texture(2, 2, |uv| v4(1., 1., 1., 0.5)) | cover, base3, filter="nearest")

base1 = texture(2, 2, |uv| 0.3)
added = blit(texture(2, 2, |uv| 0.4) | cover, base1, blend="add", filter="nearest")
maxed = blit(texture(2, 2, |uv| 0.1) | cover, base1, blend="max", filter="nearest")
"#,
    )
    .unwrap();

    let over = get_tex(&ctx, "over");
    assert_eq!(over.channels, 3);
    for px in over.pixels.iter() {
      assert!((px - 0.6).abs() < 1e-6, "over: expected 0.6, got {px}");
    }

    for px in get_tex(&ctx, "added").pixels.iter() {
      assert!((px - 0.7).abs() < 1e-6, "add: expected 0.7, got {px}");
    }
    for px in get_tex(&ctx, "maxed").pixels.iter() {
      assert!((px - 0.3).abs() < 1e-6, "max: expected 0.3, got {px}");
    }
  }

  #[test]
  fn blit_minification_uses_mips() {
    let ctx = parse_and_eval_program(
      r#"
base = texture(2, 2, |uv| 0.)
checker = texture(16, 16, |uv| (floor(uv.x * 16.) + floor(uv.y * 16.)) % 2.)
out = blit(checker | scale(0.5) | trans_global(0.25, 0.25), base)
"#,
    )
    .unwrap();

    let out = get_tex(&ctx, "out");
    // 16 checker texels minified into one base pixel: the mip chain averages to ~0.5;
    // unfiltered sampling would alias to 0 or 1
    let px = out.pixels[0];
    assert!((px - 0.5).abs() < 0.1, "expected ~0.5 from mips, got {px}");
    assert_eq!(out.pixels[3], 0., "pixel outside the footprint");
  }

  #[test]
  fn texture_transform_rotation_convention() {
    let ctx = parse_and_eval_program(
      r#"
grad = texture(2, 1, |uv| 0.25 + 0.5 * floor(uv.x * 2.))
base_h = texture(2, 1, |uv| 0.)
base_v = texture(1, 2, |uv| 0.)
flipped = blit(grad | rot(pi) | trans_global(0.5, 0.5), base_h, filter="nearest")
turned = blit(grad | rot(pi / 2.) | trans_global(0.5, 0.5), base_v, filter="nearest")
"#,
    )
    .unwrap();

    // 180°: horizontal order swaps
    let flipped = get_tex(&ctx, "flipped");
    assert_eq!(&flipped.pixels[..], &[0.75, 0.25]);

    // +90° CCW in UV coords (v-down storage): +local-x maps to +v, so the right texel
    // lands in the bottom row
    let turned = get_tex(&ctx, "turned");
    assert_eq!(&turned.pixels[..], &[0.25, 0.75]);
  }

  #[test]
  fn scatter_generator_and_seq_forms() {
    let ctx = parse_and_eval_program(
      r#"
pts = [v2(0.125, 0.125), v2(0.625, 0.125), v2(0.125, 0.625), v2(0.625, 0.625)]
base = texture(4, 4, |uv| 0.)
dot = texture(1, 1, |uv| 1.)
scattered = scatter(4, |ix| dot | scale(0.25) | trans_global(pts[ix]), base, filter="nearest")
from_seq = scatter([dot | scale(0.25) | trans_global(0.875, 0.875)], base, blend="add", filter="nearest")
"#,
    )
    .unwrap();

    let scattered = get_tex(&ctx, "scattered");
    for y in 0..4 {
      for x in 0..4 {
        let expected = if x % 2 == 0 && y % 2 == 0 { 1. } else { 0. };
        assert_eq!(scattered.pixels[y * 4 + x], expected, "scattered ({x}, {y})");
      }
    }

    let from_seq = get_tex(&ctx, "from_seq");
    assert_eq!(from_seq.pixels.iter().sum::<f32>(), 1.);
    assert_eq!(from_seq.pixels[3 * 4 + 3], 1.);
  }
}

#[cfg(test)]
mod cache_and_alpha_tests {
  use super::get_or_build_mips;
  use crate::{parse_and_eval_program, Mat4, TextureHandle, Value};
  use std::rc::Rc;

  /// Placement clones share the mip chain; swapping in new pixels invalidates it.
  #[test]
  fn mip_cache_shared_across_placement_clones() {
    let ctx = parse_and_eval_program("t = texture(8, 8, |uv| uv.x)").unwrap();
    let Value::Texture(t) = ctx.get_global("t").unwrap() else {
      panic!("expected texture");
    };
    let a = get_or_build_mips(&t);

    let placed = TextureHandle {
      transform: Mat4::identity().append_scaling(0.25),
      ..(*t).clone()
    };
    assert!(Rc::ptr_eq(&a, &get_or_build_mips(&placed)));

    let repixeled = TextureHandle {
      pixels: Rc::new(vec![0.5; 8 * 8]),
      ..(*t).clone()
    };
    assert!(!Rc::ptr_eq(&a, &get_or_build_mips(&repixeled)));
  }

  /// Transparent-green / opaque-blue seam: straight-alpha blur would bleed green into the
  /// visible blue side.
  #[test]
  fn blur_rgba_filters_premultiplied() {
    let ctx = parse_and_eval_program(
      r#"
t = texture(16, 16, |uv| v4(0., 1. - floor(uv.x * 2.), floor(uv.x * 2.), floor(uv.x * 2.)))
b = t | blur(2.)
"#,
    )
    .unwrap();
    let Value::Texture(b) = ctx.get_global("b").unwrap() else {
      panic!("expected texture");
    };
    // Pixel just inside the opaque half (x=9, mid row): green must not bleed through.
    let px = &b.pixels[(8 * 16 + 9) * 4..(8 * 16 + 9) * 4 + 4];
    assert!(px[1] < 0.01, "green bled into opaque side: {px:?}");
    assert!(px[2] > 0.9, "blue should stay saturated: {px:?}");
    assert!(px[3] > 0.4 && px[3] < 1.01, "alpha should blur normally: {px:?}");
  }
}
