use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  builtins::trace_path::expect_path,
  path::{lazy::LerpPath, Path},
  seq::EagerSeq,
  ArgRef, ErrorStack, EvalCtx, Sym, Value,
};

pub fn lerp_paths_impl(
  ctx: &EvalCtx,
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

  let lerp = |a: &Rc<Path>, b: &Rc<Path>| {
    Rc::new(Path::lazy(Rc::new(LerpPath::new(
      Rc::clone(a),
      Rc::clone(b),
      mix,
      sample_count,
    ))))
  };
  let (subs_a, subs_b) = (path_a.subpaths(), path_b.subpaths());
  if subs_a.len() == 1 && subs_b.len() == 1 {
    return Ok(Value::Path(lerp(path_a, path_b)));
  }
  if subs_a.len() != subs_b.len() {
    return Err(ErrorStack::new(format!(
      "lerp_paths: subpath counts differ ({} vs {}); multi-subpath inputs are lerped pairwise",
      subs_a.len(),
      subs_b.len()
    )));
  }
  let pairs = subs_a
    .iter()
    .zip(&subs_b)
    .map(|(a, b)| lerp(a, b))
    .collect();
  Ok(Value::Path(Rc::new(Path::group_items(
    pairs,
    path_a.fill_rule.or(path_b.fill_rule),
    ctx,
  )?)))
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
