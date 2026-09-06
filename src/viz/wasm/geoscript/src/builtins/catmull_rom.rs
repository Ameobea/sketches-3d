use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;

use crate::{
  path::{lazy::CatmullRom2D, Path},
  path_building::eval_cardinal_spline,
  ArgRef, Callable, DynamicCallable, ErrorStack, EvalCtx, Sym, Value, Vec2,
};

// ── 2D ──────────────────────────────────────────────────────────────────────

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
      .ok_or_else(|| {
        ErrorStack::new(format!(
          "catmull_rom_3d: `t` must be a number, got {t_val:?}"
        ))
      })?
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

  Ok(Value::Path(Rc::new(Path::lazy(Rc::new(CatmullRom2D {
    points,
    tension,
    closed,
  })))))
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
