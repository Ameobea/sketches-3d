use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  builtins::trace_path::expect_path,
  path::{lazy::LerpPath, Path},
  seq::EagerSeq,
  ArgRef, ErrorStack, EvalCtx, Sym, Value,
};

pub fn lerp_paths_impl(
  _ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path_a = expect_path(arg_refs[0].resolve(args, kwargs), "lerp_paths")?;
  let path_b = expect_path(arg_refs[1].resolve(args, kwargs), "lerp_paths")?;
  let mix = arg_refs[2]
    .resolve(args, kwargs)
    .as_float()
    .ok_or_else(|| ErrorStack::new("lerp_paths: `mix` must be a number"))?
    .clamp(0.0, 1.0);
  let sample_count = arg_refs[3]
    .resolve(args, kwargs)
    .as_int()
    .ok_or_else(|| ErrorStack::new("lerp_paths: `sample_count` must be an integer"))?
    as usize;

  Ok(Value::Path(Rc::new(Path::lazy(Rc::new(LerpPath::new(
    Rc::clone(path_a),
    Rc::clone(path_b),
    mix,
    sample_count,
  ))))))
}

pub fn critical_points_impl(
  _ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path = expect_path(arg_refs[0].resolve(args, kwargs), "critical_points")?;
  let values: Vec<Value> = path
    .critical_t_values()
    .into_iter()
    .map(Value::Float)
    .collect();
  Ok(Value::Sequence(Rc::new(EagerSeq {
    inner: Rc::new(values),
  })))
}
