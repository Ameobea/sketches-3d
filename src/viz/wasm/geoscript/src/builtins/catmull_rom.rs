use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;
use nalgebra::Matrix3;

use crate::{
  builtins::trace_path::{PathSampler, SubpathTopology},
  path_building::eval_cardinal_spline,
  ArgRef, Callable, DynamicCallable, ErrorStack, EvalCtx, Sym, Value, Vec2,
};

// ── 2D ──────────────────────────────────────────────────────────────────────

pub(crate) struct CatmullRomCallable2D {
  points: Vec<Vec2>,
  tension: f32,
  closed: bool,
  transform: Matrix3<f32>,
}

impl PathSampler for CatmullRomCallable2D {
  fn critical_t_values(&self) -> Vec<f32> {
    // Catmull-Rom knot joints are C1 smooth — only the range endpoints are notable.
    vec![0.0, 1.0]
  }

  fn subpath_topology(&self) -> Option<Vec<SubpathTopology>> {
    let segment_count = if self.closed {
      self.points.len()
    } else {
      self.points.len() - 1
    };
    Some(vec![SubpathTopology {
      closed: self.closed,
      segment_count,
    }])
  }

  fn transform(&self) -> &Matrix3<f32> {
    &self.transform
  }

  fn with_transform(&self, t: Matrix3<f32>) -> Box<dyn DynamicCallable> {
    Box::new(CatmullRomCallable2D {
      points: self.points.clone(),
      tension: self.tension,
      closed: self.closed,
      transform: t * self.transform,
    })
  }

  fn eval_at_raw(&self, t: f32, _ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    Ok(eval_cardinal_spline(&self.points, t, self.tension, self.closed))
  }
}

// ── 3D ──────────────────────────────────────────────────────────────────────

pub(crate) struct CatmullRomCallable3D {
  points: Vec<Vec3>,
  tension: f32,
  closed: bool,
}

impl DynamicCallable for CatmullRomCallable3D {
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
    let t_val = if !kwargs.is_empty() {
      let interned_t = ctx.interned_symbols.intern("t");
      kwargs
        .get(&interned_t)
        .ok_or_else(|| ErrorStack::new("catmull_rom_3d: unexpected keyword argument"))?
    } else {
      args
        .first()
        .ok_or_else(|| ErrorStack::new("catmull_rom_3d: expected argument `t`"))?
    };
    let t = t_val
      .as_float()
      .ok_or_else(|| ErrorStack::new(format!("catmull_rom_3d: `t` must be a number, got {t_val:?}")))?
      .clamp(0., 1.);

    let pos = eval_cardinal_spline(&self.points, t, self.tension, self.closed);
    Ok(Value::Vec3(pos))
  }

  fn get_return_type_hint(&self) -> Option<crate::ArgType> {
    Some(crate::ArgType::Vec3)
  }
}

// ── builtin impls ────────────────────────────────────────────────────────────

pub fn catmull_rom_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let points_seq = arg_refs[0]
    .resolve(args, kwargs)
    .as_sequence()
    .ok_or_else(|| ErrorStack::new("catmull_rom: `points` must be a sequence"))?;
  let tension = arg_refs[1]
    .resolve(args, kwargs)
    .as_float()
    .ok_or_else(|| ErrorStack::new("catmull_rom: `tension` must be a number"))?;
  let closed = arg_refs[2]
    .resolve(args, kwargs)
    .as_bool()
    .ok_or_else(|| ErrorStack::new("catmull_rom: `closed` must be a bool"))?;

  let points = points_seq
    .consume(ctx)
    .map(|res| match res {
      Ok(Value::Vec2(v)) => Ok(v),
      Ok(val) => Err(ErrorStack::new(format!(
        "catmull_rom: expected vec2 control points, found {val:?}"
      ))),
      Err(err) => Err(err),
    })
    .collect::<Result<Vec<Vec2>, _>>()?;

  if points.len() < 2 {
    return Err(ErrorStack::new(
      "catmull_rom: at least 2 control points are required",
    ));
  }

  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: "catmull_rom".to_owned(),
    inner: Box::new(CatmullRomCallable2D {
      points,
      tension,
      closed,
      transform: Matrix3::identity(),
    }),
  })))
}

pub fn catmull_rom_3d_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let points_seq = arg_refs[0]
    .resolve(args, kwargs)
    .as_sequence()
    .ok_or_else(|| ErrorStack::new("catmull_rom_3d: `points` must be a sequence"))?;
  let tension = arg_refs[1]
    .resolve(args, kwargs)
    .as_float()
    .ok_or_else(|| ErrorStack::new("catmull_rom_3d: `tension` must be a number"))?;
  let closed = arg_refs[2]
    .resolve(args, kwargs)
    .as_bool()
    .ok_or_else(|| ErrorStack::new("catmull_rom_3d: `closed` must be a bool"))?;

  let points = points_seq
    .consume(ctx)
    .map(|res| match res {
      Ok(Value::Vec3(v)) => Ok(v),
      Ok(val) => Err(ErrorStack::new(format!(
        "catmull_rom_3d: expected vec3 control points, found {val:?}"
      ))),
      Err(err) => Err(err),
    })
    .collect::<Result<Vec<Vec3>, _>>()?;

  if points.len() < 2 {
    return Err(ErrorStack::new(
      "catmull_rom_3d: at least 2 control points are required",
    ));
  }

  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: "catmull_rom_3d".to_owned(),
    inner: Box::new(CatmullRomCallable3D {
      points,
      tension,
      closed,
    }),
  })))
}
