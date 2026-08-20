//! `ramp` / `color_ramp`: multi-stop transfer-function builders returning callables.
//!
//! Representation is chosen from the spec at construct time, never from call counts, so
//! ramps stay pure values: native-easing ramps evaluate segments exactly (crisp step
//! edges, zero quantization); ramps with closure easings or non-linear color-space mixing
//! bake a 256-entry lerp-sampled LUT so the expensive math runs only at construct.

use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;
use nanoserde::{DeJson, SerJson};

use crate::{
  color::{linear_to_oklab, linear_to_srgb, oklab_to_linear, srgb_to_linear},
  ArgRef, ArgType, Callable, ControlKind, DynamicCallable, ErrorStack, EvalCtx, RenderedControl,
  Sym, Value, EMPTY_KWARGS,
};

const LUT_SIZE: usize = 256;

#[derive(Clone)]
pub(crate) enum Ease {
  Linear,
  Smooth,
  Smoother,
  Step,
  Custom(Rc<Callable>),
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum MixSpace {
  Linear,
  Oklab,
  Oklch,
  Srgb,
}

#[derive(Clone, Copy)]
pub(crate) enum Extend {
  Clamp,
  Repeat,
  Mirror,
}

pub(crate) struct RampSpec {
  /// Sorted ascending (stable, so duplicate positions keep author order = hard edge).
  pub positions: Vec<f32>,
  /// Scalar payloads live in `.x`.
  pub values: Vec<Vec3>,
  /// Per stop; a stop's ease governs the segment leaving it (last stop's is inert).
  pub eases: Vec<Ease>,
  pub scalar: bool,
  pub extend: Extend,
  pub space: MixSpace,
}

enum Baked {
  Exact,
  Lut(Vec<Vec3>),
}

fn parse_ease(v: &Value, fn_name: &str) -> Result<Ease, ErrorStack> {
  match v {
    Value::String(s) => match s.as_str() {
      "linear" => Ok(Ease::Linear),
      "smooth" => Ok(Ease::Smooth),
      "smoother" => Ok(Ease::Smoother),
      "step" => Ok(Ease::Step),
      other => Err(ErrorStack::new(format!(
        "{fn_name}: unknown ease {other:?}; expected \"linear\" | \"smooth\" | \"smoother\" | \"step\" or a callable"
      ))),
    },
    Value::Callable(cb) => Ok(Ease::Custom(Rc::clone(cb))),
    other => Err(ErrorStack::new(format!(
      "{fn_name}: `ease` must be a string or callable, got {other:?}"
    ))),
  }
}

fn apply_ease(ease: &Ease, t: f32, ctx: &EvalCtx) -> Result<f32, ErrorStack> {
  Ok(match ease {
    Ease::Linear => t,
    Ease::Smooth => t * t * (3. - 2. * t),
    Ease::Smoother => t * t * t * (t * (t * 6. - 15.) + 10.),
    Ease::Step => 0.,
    Ease::Custom(cb) => ctx
      .invoke_callable(cb, &[Value::Float(t)], EMPTY_KWARGS)
      .map_err(|err| err.wrap("error calling custom ramp `ease` callable"))?
      .as_float()
      .ok_or_else(|| ErrorStack::new("custom ramp `ease` callable must return a number"))?,
  })
}

fn to_space(space: MixSpace, v: Vec3) -> Vec3 {
  match space {
    MixSpace::Linear => v,
    MixSpace::Srgb => linear_to_srgb(v),
    MixSpace::Oklab => linear_to_oklab(v),
    MixSpace::Oklch => {
      let lab = linear_to_oklab(v);
      Vec3::new(lab.x, (lab.y * lab.y + lab.z * lab.z).sqrt(), lab.z.atan2(lab.y))
    }
  }
}

fn from_space(space: MixSpace, v: Vec3) -> Vec3 {
  let clamp01 = |c: Vec3| c.map(|x| x.clamp(0., 1.));
  match space {
    MixSpace::Linear => v,
    MixSpace::Srgb => clamp01(srgb_to_linear(v)),
    MixSpace::Oklab => clamp01(oklab_to_linear(v)),
    MixSpace::Oklch => clamp01(oklab_to_linear(Vec3::new(
      v.x,
      v.y * v.z.cos(),
      v.y * v.z.sin(),
    ))),
  }
}

fn mix_in_space(space: MixSpace, a: Vec3, b: Vec3, t: f32) -> Vec3 {
  match space {
    // Hue is an angle: shorter arc, and an achromatic endpoint adopts its neighbor's hue.
    MixSpace::Oklch => {
      const ACHROMATIC_C: f32 = 1e-4;
      let (mut ha, mut hb) = (a.z, b.z);
      if a.y < ACHROMATIC_C {
        ha = hb;
      }
      if b.y < ACHROMATIC_C {
        hb = ha;
      }
      let dh = (hb - ha + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
      Vec3::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t, ha + dh * t)
    }
    _ => a.lerp(&b, t),
  }
}

fn map_extend(x: f32, lo: f32, hi: f32, extend: Extend) -> f32 {
  if hi <= lo {
    return lo;
  }
  match extend {
    Extend::Clamp => crate::builtins::clampf(x, lo, hi),
    Extend::Repeat => lo + (x - lo).rem_euclid(hi - lo),
    Extend::Mirror => {
      let span = hi - lo;
      let m = (x - lo).rem_euclid(2. * span);
      lo + (span - (m - span).abs())
    }
  }
}

/// Piecewise evaluation over `vals` (which live in `space`); `u` must already be
/// extend-mapped into the stop extent.
fn eval_segments(
  spec: &RampSpec,
  vals: &[Vec3],
  u: f32,
  space: MixSpace,
  ctx: &EvalCtx,
) -> Result<Vec3, ErrorStack> {
  let ps = &spec.positions;
  let n = ps.len();
  if n == 1 {
    return Ok(vals[0]);
  }
  // `<=` puts u exactly on a (possibly duplicated) stop into the segment to its right,
  // which is what makes duplicate positions a hard edge.
  let idx = ps.partition_point(|p| *p <= u);
  if idx == 0 {
    return Ok(vals[0]);
  }
  if idx >= n {
    return Ok(vals[n - 1]);
  }
  let (p0, p1) = (ps[idx - 1], ps[idx]);
  let t = if p1 > p0 { (u - p0) / (p1 - p0) } else { 1. };
  let t = apply_ease(&spec.eases[idx - 1], t, ctx)?;
  Ok(mix_in_space(space, vals[idx - 1], vals[idx], t))
}

fn needs_lut(spec: &RampSpec) -> bool {
  let n = spec.positions.len();
  if n < 2 || spec.positions[n - 1] <= spec.positions[0] {
    return false;
  }
  let seg_eases = &spec.eases[..n - 1];
  seg_eases.iter().any(|e| matches!(e, Ease::Custom(_)))
    || (spec.space != MixSpace::Linear && seg_eases.iter().any(|e| !matches!(e, Ease::Step)))
}

fn bake(spec: &RampSpec, ctx: &EvalCtx) -> Result<Baked, ErrorStack> {
  if !needs_lut(spec) {
    return Ok(Baked::Exact);
  }
  let (lo, hi) = (spec.positions[0], *spec.positions.last().unwrap());
  let space_vals: Vec<Vec3> = spec.values.iter().map(|v| to_space(spec.space, *v)).collect();
  let mut lut = Vec::with_capacity(LUT_SIZE);
  for i in 0..LUT_SIZE {
    let u = lo + (hi - lo) * (i as f32 / (LUT_SIZE - 1) as f32);
    let v = eval_segments(spec, &space_vals, u, spec.space, ctx)?;
    lut.push(from_space(spec.space, v));
  }
  Ok(Baked::Lut(lut))
}

pub(crate) struct RampCallable {
  pub spec: RampSpec,
  baked: Baked,
}

impl RampCallable {
  fn sample(&self, x: f32, ctx: &EvalCtx) -> Result<Vec3, ErrorStack> {
    let (lo, hi) = (self.spec.positions[0], *self.spec.positions.last().unwrap());
    let u = map_extend(x, lo, hi, self.spec.extend);
    match &self.baked {
      // Exact ramps never mix in a non-linear space (`needs_lut` guarantees any non-linear
      // space here is all-step, which never interpolates), so raw linear values are correct.
      Baked::Exact => eval_segments(&self.spec, &self.spec.values, u, MixSpace::Linear, ctx),
      Baked::Lut(lut) => {
        let f = (u - lo) / (hi - lo) * (LUT_SIZE - 1) as f32;
        let i = (f as usize).min(LUT_SIZE - 2);
        let t = (f - i as f32).clamp(0., 1.);
        Ok(lut[i].lerp(&lut[i + 1], t))
      }
    }
  }
}

impl DynamicCallable for RampCallable {
  fn as_any(&self) -> &dyn std::any::Any {
    self
  }

  fn is_side_effectful(&self) -> bool {
    false
  }

  fn is_rng_dependent(&self) -> bool {
    false
  }

  fn invoke(
    &self,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let x_val = if let Some(v) = args.first() {
      v
    } else {
      let interned_x = ctx.interned_symbols.intern("x");
      kwargs
        .get(&interned_x)
        .ok_or_else(|| ErrorStack::new("ramp: expected argument `x`"))?
    };
    if let Value::Texture(tex) = x_val {
      if tex.channels != 1 {
        return Err(ErrorStack::new(format!(
          "ramp applied to a texture requires 1 channel, found {}",
          tex.channels
        )));
      }
      let out_ch = if self.spec.scalar { 1 } else { 3 };
      let src = &tex.as_planes()[0];
      let mut planes: Vec<Vec<f32>> = (0..out_ch).map(|_| Vec::with_capacity(src.len())).collect();
      for &v in src.iter() {
        let c = self.sample(v, ctx)?;
        if self.spec.scalar {
          planes[0].push(c.x);
        } else {
          planes[0].push(c.x);
          planes[1].push(c.y);
          planes[2].push(c.z);
        }
      }
      return Ok(Value::Texture(std::rc::Rc::new(crate::TextureHandle {
        storage: crate::TexStorage::from_plane_vecs(planes),
        channels: out_ch,
        mips: Default::default(),
        ..(**tex).clone()
      })));
    }
    let x = x_val
      .as_float()
      .ok_or_else(|| ErrorStack::new(format!("ramp: `x` must be a number, got {x_val:?}")))?;
    let v = self.sample(x, ctx)?;
    Ok(if self.spec.scalar {
      Value::Float(v.x)
    } else {
      Value::Vec3(v)
    })
  }

  fn get_return_type_hint(&self) -> Option<ArgType> {
    Some(if self.spec.scalar {
      ArgType::Float
    } else {
      ArgType::Vec3
    })
  }

  fn content_hash(&self, hasher: &mut dyn std::hash::Hasher) -> Option<()> {
    let spec = &self.spec;
    // Closure easings have no stable content; decide before writing any bytes.
    let ease_bytes: Vec<u8> = spec
      .eases
      .iter()
      .map(|e| {
        Some(match e {
          Ease::Linear => 0u8,
          Ease::Smooth => 1,
          Ease::Smoother => 2,
          Ease::Step => 3,
          Ease::Custom(_) => return None,
        })
      })
      .collect::<Option<_>>()?;
    hasher.write(b"ramp");
    hasher.write_u8(spec.scalar as u8);
    hasher.write_u8(match spec.extend {
      Extend::Clamp => 0,
      Extend::Repeat => 1,
      Extend::Mirror => 2,
    });
    hasher.write_u8(match spec.space {
      MixSpace::Linear => 0,
      MixSpace::Oklab => 1,
      MixSpace::Oklch => 2,
      MixSpace::Srgb => 3,
    });
    for ((pos, v), ease) in spec.positions.iter().zip(&spec.values).zip(ease_bytes) {
      hasher.write_u32(pos.to_bits());
      for c in [v.x, v.y, v.z] {
        hasher.write_u32(c.to_bits());
      }
      hasher.write_u8(ease);
    }
    Some(())
  }
}

/// (position, payload, ease) triples in author order; payload validated later.
type RawStop = (f32, Value, Ease);

fn parse_pair_seq(
  ctx: &EvalCtx,
  parts: Vec<Value>,
  default_ease: &Ease,
  fn_name: &str,
) -> Result<RawStop, ErrorStack> {
  if parts.len() != 2 && parts.len() != 3 {
    return Err(ErrorStack::new(format!(
      "{fn_name}: positioned stops must be `[pos, value]` or `[pos, value, ease]`; got {} elements",
      parts.len()
    )));
  }
  let _ = ctx;
  let pos = parts[0].as_float().ok_or_else(|| {
    ErrorStack::new(format!(
      "{fn_name}: stop position must be a number, got {:?}",
      parts[0]
    ))
  })?;
  let ease = match parts.get(2) {
    Some(v) => parse_ease(v, fn_name)?,
    None => default_ease.clone(),
  };
  Ok((pos, parts[1].clone(), ease))
}

fn parse_map_stop(
  map: &FxHashMap<String, Value>,
  default_ease: &Ease,
  fn_name: &str,
) -> Result<RawStop, ErrorStack> {
  let pos = map
    .get("pos")
    .and_then(|v| v.as_float())
    .ok_or_else(|| ErrorStack::new(format!("{fn_name}: map stops need a numeric `pos`")))?;
  let value = map
    .get("value")
    .or_else(|| map.get("val"))
    .ok_or_else(|| ErrorStack::new(format!("{fn_name}: map stops need a `value`")))?
    .clone();
  let ease = match map.get("ease") {
    Some(v) => parse_ease(v, fn_name)?,
    None => default_ease.clone(),
  };
  Ok((pos, value, ease))
}

fn parse_stops(
  ctx: &EvalCtx,
  stops_val: &Value,
  domain: (f32, f32),
  default_ease: &Ease,
  require_color: bool,
  fn_name: &str,
) -> Result<(Vec<f32>, Vec<Vec3>, Vec<Ease>, bool), ErrorStack> {
  let seq = stops_val
    .as_sequence()
    .ok_or_else(|| ErrorStack::new(format!("{fn_name}: `stops` must be a sequence")))?;
  let elems: Vec<Value> = seq.consume(ctx).collect::<Result<_, _>>()?;
  if elems.is_empty() {
    return Err(ErrorStack::new(format!("{fn_name}: `stops` is empty")));
  }

  // Bare payloads (floats/vec3s) are never sequences/maps/vec2s, so pair-likeness is
  // unambiguous per element; mixing the two forms in one list is rejected.
  let is_pairlike =
    |v: &Value| matches!(v, Value::Sequence(_) | Value::Map(_) | Value::Vec2(_));
  let explicit = elems.iter().all(is_pairlike);
  if !explicit && elems.iter().any(is_pairlike) {
    return Err(ErrorStack::new(format!(
      "{fn_name}: `stops` mixes positioned stops with bare values; use one form"
    )));
  }

  let mut stops: Vec<RawStop> = Vec::with_capacity(elems.len());
  if explicit {
    for e in &elems {
      stops.push(match e {
        Value::Vec2(v) => (v.x, Value::Float(v.y), default_ease.clone()),
        Value::Sequence(s) => {
          let parts: Vec<Value> = s.consume(ctx).collect::<Result<_, _>>()?;
          parse_pair_seq(ctx, parts, default_ease, fn_name)?
        }
        Value::Map(m) => parse_map_stop(m, default_ease, fn_name)?,
        _ => unreachable!(),
      });
    }
  } else {
    let n = elems.len();
    for (i, e) in elems.iter().enumerate() {
      let frac = if n == 1 { 0. } else { i as f32 / (n - 1) as f32 };
      stops.push((
        domain.0 + (domain.1 - domain.0) * frac,
        e.clone(),
        default_ease.clone(),
      ));
    }
  }

  if stops.iter().any(|(p, ..)| !p.is_finite()) {
    return Err(ErrorStack::new(format!(
      "{fn_name}: stop positions must be finite"
    )));
  }
  stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

  let scalar = stops.iter().all(|(_, v, _)| v.as_float().is_some());
  if require_color && scalar {
    return Err(ErrorStack::new(format!(
      "{fn_name}: stop values must be vec3 colors (linear RGB); use `ramp` for scalar stops"
    )));
  }
  let mut positions = Vec::with_capacity(stops.len());
  let mut values = Vec::with_capacity(stops.len());
  let mut eases = Vec::with_capacity(stops.len());
  for (pos, val, ease) in stops {
    let v = if scalar {
      Vec3::new(val.as_float().unwrap(), 0., 0.)
    } else {
      match val {
        Value::Vec3(v) => v,
        other => {
          return Err(ErrorStack::new(format!(
            "{fn_name}: stop values must be all numbers or all vec3s, got {other:?}"
          )))
        }
      }
    };
    positions.push(pos);
    values.push(v);
    eases.push(ease);
  }
  Ok((positions, values, eases, scalar))
}

fn parse_domain(v: &Value, ctx: &EvalCtx, fn_name: &str) -> Result<(f32, f32), ErrorStack> {
  let err = || {
    ErrorStack::new(format!(
      "{fn_name}: `domain` must be `[lo, hi]` (or a vec2) with lo < hi"
    ))
  };
  let (lo, hi) = match v {
    Value::Vec2(v) => (v.x, v.y),
    Value::Sequence(s) => {
      let parts: Vec<Value> = s.consume(ctx).collect::<Result<_, _>>()?;
      if parts.len() != 2 {
        return Err(err());
      }
      (
        parts[0].as_float().ok_or_else(err)?,
        parts[1].as_float().ok_or_else(err)?,
      )
    }
    _ => return Err(err()),
  };
  if !(lo < hi) {
    return Err(err());
  }
  Ok((lo, hi))
}

fn parse_extend(v: &Value, fn_name: &str) -> Result<Extend, ErrorStack> {
  match v.as_str() {
    Some("clamp") => Ok(Extend::Clamp),
    Some("repeat") => Ok(Extend::Repeat),
    Some("mirror") => Ok(Extend::Mirror),
    _ => Err(ErrorStack::new(format!(
      "{fn_name}: `extend` must be \"clamp\" | \"repeat\" | \"mirror\", got {v:?}"
    ))),
  }
}

fn construct_ramp(
  ctx: &EvalCtx,
  stops_val: &Value,
  domain: (f32, f32),
  extend: Extend,
  default_ease: &Ease,
  space: Option<MixSpace>,
  fn_name: &'static str,
) -> Result<Value, ErrorStack> {
  let require_color = space.is_some();
  let (positions, values, eases, scalar) =
    parse_stops(ctx, stops_val, domain, default_ease, require_color, fn_name)?;
  let spec = RampSpec {
    positions,
    values,
    eases,
    scalar,
    extend,
    space: space.unwrap_or(MixSpace::Linear),
  };
  let baked = bake(&spec, ctx)?;
  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: fn_name.to_owned(),
    inner: Box::new(RampCallable { spec, baked }),
  })))
}

fn build_ramp(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  space: Option<MixSpace>,
  fn_name: &'static str,
) -> Result<Value, ErrorStack> {
  let stops_val = arg_refs[0].resolve(args, kwargs);
  let domain = parse_domain(&arg_refs[1].resolve(args, kwargs), ctx, fn_name)?;
  let extend = parse_extend(&arg_refs[2].resolve(args, kwargs), fn_name)?;
  let default_ease = parse_ease(&arg_refs[3].resolve(args, kwargs), fn_name)?;
  construct_ramp(ctx, &stops_val, domain, extend, &default_ease, space, fn_name)
}

pub fn ramp_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  build_ramp(ctx, arg_refs, args, kwargs, None, "ramp")
}

pub fn color_ramp_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let space = match arg_refs[4].resolve(args, kwargs).as_str() {
    Some("oklab") => MixSpace::Oklab,
    Some("oklch") => MixSpace::Oklch,
    Some("linear") => MixSpace::Linear,
    Some("srgb") => MixSpace::Srgb,
    other => {
      return Err(ErrorStack::new(format!(
        "color_ramp: `space` must be \"oklab\" | \"oklch\" | \"linear\" | \"srgb\", got {other:?}"
      )))
    }
  };
  build_ramp(ctx, arg_refs, args, kwargs, Some(space), "color_ramp")
}

pub fn remap_impl(
  _ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let f = |ix: usize, name: &str| -> Result<f32, ErrorStack> {
    arg_refs[ix].resolve(args, kwargs).as_float().ok_or_else(|| {
      ErrorStack::new(format!("remap: `{name}` must be a number"))
    })
  };
  let (in_lo, in_hi) = (f(0, "in_lo")?, f(1, "in_hi")?);
  let (out_lo, out_hi) = (f(2, "out_lo")?, f(3, "out_hi")?);
  let x_val = arg_refs[4].resolve(args, kwargs);
  let clamp = arg_refs[5]
    .resolve(args, kwargs)
    .as_bool()
    .ok_or_else(|| ErrorStack::new("remap: `clamp` must be a bool"))?;

  let map1 = |v: f32| {
    let mut t = if in_hi != in_lo {
      (v - in_lo) / (in_hi - in_lo)
    } else {
      0.
    };
    if clamp {
      t = t.clamp(0., 1.);
    }
    out_lo + (out_hi - out_lo) * t
  };
  match &x_val {
    Value::Vec3(v) => Ok(Value::Vec3(v.map(map1))),
    Value::Texture(t) => Ok(super::texture::texture_map_unary(t, map1)),
    v => Ok(Value::Float(map1(v.as_float().ok_or_else(|| {
      ErrorStack::new(format!("remap: `x` must be a number or vec3, got {v:?}"))
    })?))),
  }
}

pub fn srgb_impl(
  _ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let encoded = if def_ix == 0 {
    let hex = arg_refs[0]
      .resolve(args, kwargs)
      .as_int()
      .ok_or_else(|| ErrorStack::new("srgb: `hex` must be an integer like 0xC97B4A"))?;
    if !(0..=0xFFFFFF).contains(&hex) {
      return Err(ErrorStack::new(format!(
        "srgb: `hex` must be in 0x000000..=0xFFFFFF, got {hex:#x}"
      )));
    }
    Vec3::new(
      ((hex >> 16) & 0xFF) as f32 / 255.,
      ((hex >> 8) & 0xFF) as f32 / 255.,
      (hex & 0xFF) as f32 / 255.,
    )
  } else {
    let f = |ix: usize, name: &str| -> Result<f32, ErrorStack> {
      Ok(
        arg_refs[ix]
          .resolve(args, kwargs)
          .as_float()
          .ok_or_else(|| ErrorStack::new(format!("srgb: `{name}` must be a number in [0, 1]")))?
          .clamp(0., 1.),
      )
    };
    Vec3::new(f(0, "r")?, f(1, "g")?, f(2, "b")?)
  };
  Ok(Value::Vec3(srgb_to_linear(encoded)))
}

// ── input_ramp / input_color_ramp control wire ───────────────────────────────

#[derive(SerJson, DeJson)]
pub struct RampStopWire {
  pub pos: f32,
  /// 1 element for scalar ramps, 3 (linear RGB) for color ramps.
  pub value: Vec<f32>,
  pub ease: String,
}

#[derive(SerJson, DeJson)]
pub struct RampSpecWire {
  pub scalar: bool,
  pub stops: Vec<RampStopWire>,
  pub extend: String,
  pub space: String,
}

fn as_ramp(v: &Value) -> Option<&RampCallable> {
  match v {
    Value::Callable(cb) => match &**cb {
      Callable::Dynamic { inner, .. } => inner.as_any().downcast_ref::<RampCallable>(),
      _ => None,
    },
    _ => None,
  }
}

fn spec_to_wire(spec: &RampSpec) -> Option<RampSpecWire> {
  let ease_name = |e: &Ease| -> Option<&'static str> {
    Some(match e {
      Ease::Linear => "linear",
      Ease::Smooth => "smooth",
      Ease::Smoother => "smoother",
      Ease::Step => "step",
      Ease::Custom(_) => return None,
    })
  };
  let stops = spec
    .positions
    .iter()
    .zip(&spec.values)
    .zip(&spec.eases)
    .map(|((pos, v), ease)| {
      Some(RampStopWire {
        pos: *pos,
        value: if spec.scalar {
          vec![v.x]
        } else {
          vec![v.x, v.y, v.z]
        },
        ease: ease_name(ease)?.to_owned(),
      })
    })
    .collect::<Option<Vec<_>>>()?;
  Some(RampSpecWire {
    scalar: spec.scalar,
    stops,
    extend: match spec.extend {
      Extend::Clamp => "clamp",
      Extend::Repeat => "repeat",
      Extend::Mirror => "mirror",
    }
    .to_owned(),
    space: match spec.space {
      MixSpace::Linear => "linear",
      MixSpace::Oklab => "oklab",
      MixSpace::Oklch => "oklch",
      MixSpace::Srgb => "srgb",
    }
    .to_owned(),
  })
}

/// Spec JSON for a ramp value, for the host's control editor. `None` when the value isn't
/// a ramp or uses closure easings (which can't cross the wire).
pub fn ramp_control_value_json(v: &Value) -> Option<String> {
  spec_to_wire(&as_ramp(v)?.spec).map(|w| w.serialize_json())
}

/// Builds a ramp value from editor-injected spec JSON.
pub fn ramp_value_from_wire_json(json: &str, ctx: &EvalCtx) -> Result<Value, ErrorStack> {
  let wire = RampSpecWire::deserialize_json(json)
    .map_err(|err| ErrorStack::new(format!("invalid ramp control JSON: {err}")))?;
  if wire.stops.is_empty() {
    return Err(ErrorStack::new("ramp control has no stops"));
  }
  let expected_w = if wire.scalar { 1 } else { 3 };
  let mut stops: Vec<(f32, Vec3, Ease)> = Vec::with_capacity(wire.stops.len());
  for s in &wire.stops {
    if s.value.len() != expected_w || !s.pos.is_finite() {
      return Err(ErrorStack::new("malformed ramp control stop"));
    }
    let v = if wire.scalar {
      Vec3::new(s.value[0], 0., 0.)
    } else {
      Vec3::new(s.value[0], s.value[1], s.value[2])
    };
    stops.push((s.pos, v, parse_ease(&Value::String(s.ease.clone()), "ramp control")?));
  }
  stops.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
  let spec = RampSpec {
    positions: stops.iter().map(|s| s.0).collect(),
    values: stops.iter().map(|s| s.1).collect(),
    eases: stops.iter().map(|s| s.2.clone()).collect(),
    scalar: wire.scalar,
    extend: parse_extend(&Value::String(wire.extend.clone()), "ramp control")?,
    space: match wire.space.as_str() {
      "linear" => MixSpace::Linear,
      "oklab" => MixSpace::Oklab,
      "oklch" => MixSpace::Oklch,
      "srgb" => MixSpace::Srgb,
      other => {
        return Err(ErrorStack::new(format!(
          "ramp control has unknown space {other:?}"
        )))
      }
    },
  };
  let baked = bake(&spec, ctx)?;
  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: "input_ramp".to_owned(),
    inner: Box::new(RampCallable { spec, baked }),
  })))
}

pub(crate) fn input_ramp_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  color: bool,
) -> Result<Value, ErrorStack> {
  let fn_name: &'static str = if color { "input_color_ramp" } else { "input_ramp" };
  let c = super::input_common(ctx, arg_refs, args, kwargs, 2)?;
  let expected_scalar = !color;

  let injected = c
    .injected
    .filter(|v| as_ramp(v).is_some_and(|r| r.spec.scalar == expected_scalar));
  let value = match injected {
    Some(v) => v,
    None => {
      let default = arg_refs[1].resolve(args, kwargs);
      match &default {
        Value::Callable(_) => {
          let r = as_ramp(&default).ok_or_else(|| {
            ErrorStack::new(format!(
              "{fn_name}: `default` callable must be built by `ramp`/`color_ramp`"
            ))
          })?;
          if r.spec.scalar != expected_scalar {
            return Err(ErrorStack::new(format!(
              "{fn_name}: `default` has the wrong payload type (scalar vs color)"
            )));
          }
          default.clone()
        }
        Value::Sequence(_) => construct_ramp(
          ctx,
          &default,
          (0., 1.),
          Extend::Clamp,
          &Ease::Linear,
          if color { Some(MixSpace::Oklab) } else { None },
          fn_name,
        )?,
        other => {
          return Err(ErrorStack::new(format!(
            "{fn_name}: `default` must be a stop list or a built ramp, got {other:?}"
          )))
        }
      }
    }
  };

  if ramp_control_value_json(&value).is_none() {
    return Err(ErrorStack::new(format!(
      "{fn_name}: control ramps must use named easings — closures can't be serialized for the editor"
    )));
  }

  ctx.rendered_controls.push(RenderedControl {
    source_module: c.module,
    handle_id: c.handle_id,
    kind: ControlKind::Ramp,
    label: c.label,
    current_value: value.clone(),
    min: None,
    max: None,
    step: None,
    style: None,
    options: Vec::new(),
    histogram: None,
  });
  Ok(value)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parse_and_eval_program;

  fn eval_f(src: &str) -> f32 {
    let ctx = parse_and_eval_program(src).unwrap();
    ctx.get_global("out").unwrap().as_float().unwrap()
  }

  fn eval_v3(src: &str) -> Vec3 {
    let ctx = parse_and_eval_program(src).unwrap();
    match ctx.get_global("out").unwrap() {
      Value::Vec3(v) => v,
      other => panic!("expected vec3, got {other:?}"),
    }
  }

  #[test]
  fn scalar_ramp_pairs_pipe_and_extend() {
    assert_eq!(eval_f("r = ramp([[0., 0.], [1., 10.]])\nout = 0.25 | r"), 2.5);
    // clamp (default) holds ends; repeat wraps; mirror reflects
    assert_eq!(eval_f("r = ramp([[0., 0.], [1., 10.]])\nout = r(2.)"), 10.);
    assert_eq!(
      eval_f("r = ramp([[0., 0.], [1., 10.]], extend=\"repeat\")\nout = r(1.25)"),
      2.5
    );
    assert_eq!(
      eval_f("r = ramp([[0., 0.], [1., 10.]], extend=\"mirror\")\nout = r(1.25)"),
      7.5
    );
  }

  #[test]
  fn bare_values_even_spacing_over_domain() {
    assert_eq!(
      eval_f("r = ramp([0., 10., 20.], domain=[-1., 1.])\nout = r(0.)"),
      10.
    );
    assert_eq!(
      eval_f("r = ramp([0., 10., 20.], domain=[-1., 1.])\nout = r(0.5)"),
      15.
    );
  }

  #[test]
  fn step_ease_and_duplicate_position_hard_edges() {
    let src = "r = ramp([[0., 1.], [0.5, 2.], [1., 3.]], ease=\"step\")\n";
    assert_eq!(eval_f(&format!("{src}out = r(0.49)")), 1.);
    assert_eq!(eval_f(&format!("{src}out = r(0.5)")), 2.);
    assert_eq!(eval_f(&format!("{src}out = r(0.99)")), 2.);
    assert_eq!(eval_f(&format!("{src}out = r(1.)")), 3.);

    let dup = "r = ramp([[0., 0.], [0.5, 5.], [0.5, 50.], [1., 100.]])\n";
    assert_eq!(eval_f(&format!("{dup}out = r(0.5)")), 50.);
    assert!((eval_f(&format!("{dup}out = r(0.4999)")) - 5.).abs() < 0.01);
    assert_eq!(eval_f(&format!("{dup}out = r(0.75)")), 75.);
  }

  #[test]
  fn per_stop_ease_via_triple_and_map_forms() {
    let src = "r = ramp([[0., 0., \"step\"], [0.5, 5.], [1., 10.]])\n";
    assert_eq!(eval_f(&format!("{src}out = r(0.25)")), 0.);
    assert_eq!(eval_f(&format!("{src}out = r(0.75)")), 7.5);

    let src = "r = ramp([{pos: 0., value: 0., ease: \"step\"}, {pos: 0.5, value: 5.}, {pos: 1., value: 10.}])\n";
    assert_eq!(eval_f(&format!("{src}out = r(0.25)")), 0.);
    assert_eq!(eval_f(&format!("{src}out = r(0.75)")), 7.5);
  }

  #[test]
  fn custom_ease_closure_lut_matches_exact_linear() {
    for x in [0., 0.1, 0.37, 0.5, 0.82, 1.] {
      let exact = eval_f(&format!("r = ramp([[0., 0.], [1., 10.]])\nout = r({x})"));
      let lut = eval_f(&format!(
        "r = ramp([[0., 0.], [1., 10.]], ease=|t| t)\nout = r({x})"
      ));
      // Lerp-sampled LUT of a linear ramp lies on the same line.
      assert!((exact - lut).abs() < 1e-4, "x={x}: {exact} vs {lut}");
    }
  }

  #[test]
  fn vec3_scalar_split_and_smooth() {
    let v = eval_v3("r = ramp([[0., vec3(0., 1., 2.)], [1., vec3(10., 11., 12.)]])\nout = r(0.5)");
    assert_eq!((v.x, v.y, v.z), (5., 6., 7.));

    let mid = eval_f("r = ramp([[0., 0.], [1., 1.]], ease=\"smooth\")\nout = r(0.25)");
    assert!((mid - 0.15625).abs() < 1e-5);
  }

  #[test]
  fn color_ramp_spaces() {
    // Black -> white in OKLAB: midpoint has L = 0.5, i.e. linear g = 0.125 — much darker
    // than the linear-space midpoint 0.5.
    let ok = eval_v3("r = color_ramp([[0., vec3(0.)], [1., vec3(1.)]])\nout = r(0.5)");
    assert!((ok.x - 0.125).abs() < 0.01, "{ok:?}");
    let lin =
      eval_v3("r = color_ramp([[0., vec3(0.)], [1., vec3(1.)]], space=\"linear\")\nout = r(0.5)");
    assert!((lin.x - 0.5).abs() < 1e-4, "{lin:?}");
    // Legacy sRGB-space mixing: decode(0.5) ≈ 0.2140.
    let srgb_mid =
      eval_v3("r = color_ramp([[0., vec3(0.)], [1., vec3(1.)]], space=\"srgb\")\nout = r(0.5)");
    assert!((srgb_mid.x - 0.2140).abs() < 0.01, "{srgb_mid:?}");

    // Red -> blue: OKLCH keeps chroma through the middle; OKLAB passes near neutral.
    let mid_oklab = eval_v3(
      "r = color_ramp([[0., vec3(1., 0., 0.)], [1., vec3(0., 0., 1.)]])\nout = r(0.5)",
    );
    let mid_oklch = eval_v3(
      "r = color_ramp([[0., vec3(1., 0., 0.)], [1., vec3(0., 0., 1.)]], space=\"oklch\")\nout = r(0.5)",
    );
    let chroma = |v: Vec3| {
      let lab = linear_to_oklab(v);
      (lab.y * lab.y + lab.z * lab.z).sqrt()
    };
    assert!(
      chroma(mid_oklch) > chroma(mid_oklab) * 1.5,
      "oklch {:?} vs oklab {:?}",
      chroma(mid_oklch),
      chroma(mid_oklab)
    );
  }

  #[test]
  fn input_ramp_defaults_and_control_registration() {
    let ctx =
      parse_and_eval_program("r = input_ramp(\"amt\", default=[[0., 0.], [1., 10.]])\nout = r(0.3)")
        .unwrap();
    assert_eq!(ctx.get_global("out").unwrap().as_float().unwrap(), 3.);
    let controls = ctx.rendered_controls.inner.borrow();
    assert_eq!(controls.len(), 1);
    assert!(matches!(controls[0].kind, ControlKind::Ramp));
    let json = ramp_control_value_json(&controls[0].current_value).unwrap();
    assert!(json.contains("\"scalar\":true") || json.contains("\"scalar\": true"), "{json}");

    // Built-ramp default + payload-type mismatch rejection.
    let ctx = parse_and_eval_program(
      "r = input_color_ramp(\"shade\", default=color_ramp([[0., vec3(0.)], [1., vec3(1.)]], space=\"linear\"))\nout = r(0.5)",
    )
    .unwrap();
    match ctx.get_global("out").unwrap() {
      Value::Vec3(v) => assert!((v.x - 0.5).abs() < 1e-4),
      other => panic!("expected vec3, got {other:?}"),
    }
    assert!(parse_and_eval_program(
      "input_color_ramp(\"shade\", default=[[0., 0.], [1., 1.]])"
    )
    .is_err());
  }

  #[test]
  fn remap_and_srgb() {
    assert_eq!(eval_f("out = 5. | remap(0., 10., 0., 1.)"), 0.5);
    assert_eq!(eval_f("out = 15. | remap(0., 10., 0., 1.)"), 1.5);
    assert_eq!(eval_f("out = 15. | remap(0., 10., 0., 1., clamp=true)"), 1.);

    let white = eval_v3("out = srgb(0xFFFFFF)");
    assert!((white - Vec3::new(1., 1., 1.)).norm() < 1e-5);
    let gray = eval_v3("out = srgb(0x808080)");
    assert!((gray.x - 0.2158).abs() < 0.001, "{gray:?}");
    let gray2 = eval_v3("out = srgb(0.5, 0.5, 0.5)");
    assert!((gray2.x - 0.2140).abs() < 0.001, "{gray2:?}");
  }
}
