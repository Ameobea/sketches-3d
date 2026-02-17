use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  builtins::trace_path::{as_path_sampler, normalize_guides, PathSampler},
  seq::EagerSeq,
  ArgRef, Callable, ErrorStack, EvalCtx, Sym, Value, Vec2, EMPTY_KWARGS,
};

pub(crate) struct LerpPathCallable {
  path_a: Rc<Callable>,
  path_b: Rc<Callable>,
  mix: f32,
  interned_t_kwarg: Sym,
  merged_critical_points: Vec<f32>,
}

impl PathSampler for LerpPathCallable {
  fn interned_t_kwarg(&self) -> Sym {
    self.interned_t_kwarg
  }

  fn critical_t_values(&self) -> Vec<f32> {
    self.merged_critical_points.clone()
  }

  fn eval_at(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    if self.mix <= 0.0 {
      let val_a = ctx
        .invoke_callable(&self.path_a, &[Value::Float(t)], EMPTY_KWARGS)
        .map_err(|e| e.wrap("Error invoking path_a in lerp_paths"))?;
      return val_a
        .as_vec2()
        .copied()
        .ok_or_else(|| ErrorStack::new("lerp_paths: path_a did not return a vec2"));
    }
    if self.mix >= 1.0 {
      let val_b = ctx
        .invoke_callable(&self.path_b, &[Value::Float(t)], EMPTY_KWARGS)
        .map_err(|e| e.wrap("Error invoking path_b in lerp_paths"))?;
      return val_b
        .as_vec2()
        .copied()
        .ok_or_else(|| ErrorStack::new("lerp_paths: path_b did not return a vec2"));
    }

    let val_a = ctx
      .invoke_callable(&self.path_a, &[Value::Float(t)], EMPTY_KWARGS)
      .map_err(|e| e.wrap("Error invoking path_a in lerp_paths"))?;
    let val_b = ctx
      .invoke_callable(&self.path_b, &[Value::Float(t)], EMPTY_KWARGS)
      .map_err(|e| e.wrap("Error invoking path_b in lerp_paths"))?;

    let a = *val_a
      .as_vec2()
      .ok_or_else(|| ErrorStack::new("lerp_paths: path_a did not return a vec2"))?;
    let b = *val_b
      .as_vec2()
      .ok_or_else(|| ErrorStack::new("lerp_paths: path_b did not return a vec2"))?;

    Ok(a + (b - a) * self.mix)
  }
}

pub fn lerp_paths_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path_a = arg_refs[0]
    .resolve(args, kwargs)
    .as_callable()
    .ok_or_else(|| ErrorStack::new("lerp_paths: `path_a` must be a callable"))?;
  let path_b = arg_refs[1]
    .resolve(args, kwargs)
    .as_callable()
    .ok_or_else(|| ErrorStack::new("lerp_paths: `path_b` must be a callable"))?;
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

  let cps_a = as_path_sampler(&path_a).map(|s| s.critical_t_values());
  let cps_b = as_path_sampler(&path_b).map(|s| s.critical_t_values());

  let merged_critical_points = match (cps_a, cps_b) {
    (Some(mut a), Some(mut b)) => {
      a.append(&mut b);
      normalize_guides(&a)
    }
    (Some(a), None) => normalize_guides(&a),
    (None, Some(b)) => normalize_guides(&b),
    (None, None) => {
      let count = sample_count + 1;
      if count <= 1 {
        vec![0., 1.]
      } else {
        let denom = (count - 1) as f32;
        (0..count).map(|i| i as f32 / denom).collect()
      }
    }
  };

  let interned_t_kwarg = ctx.interned_symbols.intern("t");

  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: "lerp_paths".to_owned(),
    inner: Box::new(LerpPathCallable {
      path_a: Rc::clone(&path_a),
      path_b: Rc::clone(&path_b),
      mix,
      interned_t_kwarg,
      merged_critical_points,
    }),
  })))
}

pub fn critical_points_impl(
  _ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path = arg_refs[0]
    .resolve(args, kwargs)
    .as_callable()
    .ok_or_else(|| ErrorStack::new("critical_points: `path` must be a callable"))?;

  let sampler = as_path_sampler(&path).ok_or_else(|| {
    ErrorStack::new(
      "critical_points: argument must be a path sampler (e.g. from trace_path, offset_path, \
       lerp_paths). Generic callables do not have topology information.",
    )
  })?;

  let points = sampler.critical_t_values();
  let values: Vec<Value> = points.into_iter().map(Value::Float).collect();
  Ok(Value::Sequence(Rc::new(EagerSeq { inner: values })))
}
