use fxhash::FxHashMap;

use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use crate::builtins::path_critical_points::{
  collect_vertex_set, collect_vertex_set_multi, detect_critical_points, CriticalPointConfig,
  VertexSet,
};
#[cfg(target_arch = "wasm32")]
use crate::builtins::trace_path::{
  as_path_sampler, as_path_tracer, polylines_to_draw_commands, sample_path_subpaths, FillRule,
  PathTracerCallable,
};
use crate::{ArgRef, ErrorStack, EvalCtx, Sequence, Sym, Value, EMPTY_KWARGS};
#[cfg(target_arch = "wasm32")]
use crate::{Callable, Vec2};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasm/clipper2/clipper2")]
extern "C" {
  fn clipper2_get_is_loaded() -> bool;
  // op: 0=union 1=intersect 2=difference 3=xor 4=self-union (clip ignored)
  fn clipper2_boolean_flat(
    op: u32,
    fill_rule: u32,
    subject_coords: &[f32],
    subject_path_lengths: &[u32],
    clip_coords: &[f32],
    clip_path_lengths: &[u32],
  );
  fn clipper2_get_output_coords_f32() -> Vec<f32>;
  fn clipper2_get_output_path_lengths_flat() -> Vec<u32>;
  fn clipper2_clear_output_flat();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasm/cgal/cgal")]
extern "C" {
  fn cgal_get_is_loaded() -> bool;
  fn cgal_path_boolean_2d(
    subject_coords: &[f32],
    subject_path_lengths: &[u32],
    clip_coords: &[f32],
    clip_path_lengths: &[u32],
    op: u32,
  ) -> bool;
  fn cgal_get_path_boolean_2d_coords() -> Vec<f32>;
  fn cgal_get_path_boolean_2d_path_lengths() -> Vec<u32>;
  fn cgal_clear_path_boolean_2d_output();
  fn cgal_get_last_error() -> Option<String>;
}

/// Memoizes boolean results keyed on the exact sampled input geometry.  Compositions like
/// rail_sweep `dynamic_profile` closures re-run identical boolean chains once per spine
/// sample; deterministic sampling makes the coord bit patterns identical, so this collapses
/// hundreds of clipper round-trips into one per unique input.  Never invalidated (results
/// are pure); byte-bounded via `flat_memo`.
#[cfg(target_arch = "wasm32")]
mod bool_result_cache {
  use std::cell::RefCell;

  use super::BooleanResult;
  use crate::builtins::flat_memo::{polylines_bytes, push_f32_bits, FlatMemoCache};

  thread_local! {
    static CACHE: RefCell<FlatMemoCache<BooleanResult>> = RefCell::new(FlatMemoCache::default());
  }

  pub fn build_key(
    op_discriminant: u32,
    fill_rule: u32,
    subject_coords: &[f32],
    subject_path_lengths: &[u32],
    clip_coords: &[f32],
    clip_path_lengths: &[u32],
  ) -> Vec<u32> {
    let mut key = Vec::with_capacity(
      4 + subject_path_lengths.len()
        + clip_path_lengths.len()
        + subject_coords.len()
        + clip_coords.len(),
    );
    key.push(op_discriminant);
    key.push(fill_rule);
    key.push(subject_path_lengths.len() as u32);
    key.push(clip_path_lengths.len() as u32);
    key.extend_from_slice(subject_path_lengths);
    key.extend_from_slice(clip_path_lengths);
    push_f32_bits(&mut key, subject_coords);
    push_f32_bits(&mut key, clip_coords);
    key
  }

  pub fn get(key: &[u32]) -> Option<BooleanResult> {
    CACHE.with(|c| c.borrow().get(key))
  }

  pub fn insert(key: Vec<u32>, result: &BooleanResult) {
    let val_bytes = polylines_bytes(&result.paths) + result.critical_t_values.len() * 4;
    CACHE.with(|c| c.borrow_mut().insert(key, result, val_bytes));
  }
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, PartialEq, Eq)]
enum BooleanEngine {
  Clipper,
  Cgal,
}

#[cfg(target_arch = "wasm32")]
fn parse_engine(val: &Value, fn_name: &str) -> Result<BooleanEngine, ErrorStack> {
  match val {
    Value::Nil => Ok(BooleanEngine::Clipper),
    Value::String(s) => match s.as_str() {
      "clipper" | "clipper2" => Ok(BooleanEngine::Clipper),
      "cgal" => Ok(BooleanEngine::Cgal),
      other => Err(ErrorStack::new(format!(
        "Invalid `engine` for `{fn_name}`; expected \"clipper\" or \"cgal\", found: {other:?}"
      ))),
    },
    other => Err(ErrorStack::new(format!(
      "Invalid `engine` for `{fn_name}`; expected string, found: {other:?}"
    ))),
  }
}

#[cfg(target_arch = "wasm32")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
  Union,
  Intersect,
  Difference,
  Xor,
}

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
struct BooleanResult {
  paths: Vec<Vec<Vec2>>,
  critical_t_values: Vec<f32>,
}

/// Critical t-values for the boolean output in **global** t-space (the tracer walks its subpaths
/// concatenated by arc length). `detect_critical_points` reports per-path t in each path's own
/// `[0, 1]`, so for multi-subpath output each path's values are remapped by its closed-perimeter
/// arc-length offset — mirroring `offset_path::global_critical_points`, but keeping the boolean's
/// `pre_op_vertices` so op-created corners stay critical. All boolean outputs are closed.
#[cfg(target_arch = "wasm32")]
fn global_critical_points(paths: &[Vec<Vec2>], pre_op_vertices: &VertexSet) -> Vec<f32> {
  let closed_len = |p: &[Vec2]| -> f32 {
    let n = p.len();
    (0..n).map(|i| (p[(i + 1) % n] - p[i]).norm()).sum()
  };
  let lengths: Vec<f32> = paths.iter().map(|p| closed_len(p)).collect();
  let total: f32 = lengths.iter().sum();
  if total <= 1e-10 {
    return Vec::new();
  }

  let config = CriticalPointConfig::default();
  let mut out: Vec<f32> = Vec::new();
  let mut offset = 0.0f32;
  for (path, &len) in paths.iter().zip(&lengths) {
    for t_local in
      detect_critical_points(std::slice::from_ref(path), &config, Some(pre_op_vertices))
    {
      out.push(((offset + t_local * len) / total).clamp(0.0, 1.0));
    }
    offset += len;
  }
  out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
  out.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
  out
}

#[cfg(target_arch = "wasm32")]
fn run_clipper_boolean(
  subject_coords: &[f32],
  subject_path_lengths: &[u32],
  clip_coords: &[f32],
  clip_path_lengths: &[u32],
  fill_rule: u32,
  op: BooleanOp,
) -> BooleanResult {
  if subject_coords.is_empty() || subject_path_lengths.is_empty() {
    return BooleanResult {
      paths: Vec::new(),
      critical_t_values: Vec::new(),
    };
  }
  if op == BooleanOp::Union
    && subject_coords == clip_coords
    && subject_path_lengths == clip_path_lengths
  {
    return run_clipper_self_union(subject_coords, subject_path_lengths, fill_rule);
  }
  let pre_op_vertices = collect_vertex_set_multi(subject_coords, clip_coords);
  let op_code = match op {
    BooleanOp::Union => 0,
    BooleanOp::Intersect => 1,
    BooleanOp::Difference => 2,
    BooleanOp::Xor => 3,
  };
  clipper2_boolean_flat(
    op_code,
    fill_rule,
    subject_coords,
    subject_path_lengths,
    clip_coords,
    clip_path_lengths,
  );
  read_clipper_output(pre_op_vertices)
}

/// Unions every subject path in one pass (Clipper2 op 4), so a whole set of pieces costs one
/// sweep instead of a chain of pairwise unions.
#[cfg(target_arch = "wasm32")]
fn run_clipper_self_union(coords: &[f32], path_lengths: &[u32], fill_rule: u32) -> BooleanResult {
  if coords.is_empty() || path_lengths.is_empty() {
    return BooleanResult {
      paths: Vec::new(),
      critical_t_values: Vec::new(),
    };
  }
  let pre_op_vertices = collect_vertex_set(coords);
  clipper2_boolean_flat(4, fill_rule, coords, path_lengths, &[], &[]);
  read_clipper_output(pre_op_vertices)
}

#[cfg(target_arch = "wasm32")]
fn read_clipper_output(pre_op_vertices: VertexSet) -> BooleanResult {
  let out_coords = clipper2_get_output_coords_f32();
  let out_lengths = clipper2_get_output_path_lengths_flat();
  clipper2_clear_output_flat();
  let paths = paths_from_flat(&out_coords, &out_lengths);
  let critical_t_values = global_critical_points(&paths, &pre_op_vertices);
  BooleanResult {
    paths,
    critical_t_values,
  }
}

#[cfg(target_arch = "wasm32")]
fn paths_from_flat(coords: &[f32], lengths: &[u32]) -> Vec<Vec<Vec2>> {
  let mut paths = Vec::with_capacity(lengths.len());
  let mut coord_ix = 0usize;
  for &len in lengths {
    let mut path = Vec::with_capacity(len as usize);
    for _ in 0..len {
      if coord_ix + 1 >= coords.len() {
        break;
      }
      path.push(Vec2::new(coords[coord_ix], coords[coord_ix + 1]));
      coord_ix += 2;
    }
    if path.len() >= 2 {
      paths.push(path);
    }
  }
  paths
}

#[cfg(target_arch = "wasm32")]
fn paths_to_flat(paths: &[Vec<Vec2>]) -> (Vec<f32>, Vec<u32>) {
  let mut coords = Vec::with_capacity(paths.iter().map(|p| p.len() * 2).sum());
  let lengths = paths.iter().map(|p| p.len() as u32).collect();
  for p in paths {
    for pt in p {
      coords.push(pt.x);
      coords.push(pt.y);
    }
  }
  (coords, lengths)
}

#[cfg(target_arch = "wasm32")]
fn run_cgal_boolean(
  subject_coords: &[f32],
  subject_path_lengths: &[u32],
  clip_coords: &[f32],
  clip_path_lengths: &[u32],
  op: BooleanOp,
  fn_name: &str,
) -> Result<BooleanResult, ErrorStack> {
  if subject_coords.is_empty() || subject_path_lengths.is_empty() {
    return Ok(BooleanResult {
      paths: Vec::new(),
      critical_t_values: Vec::new(),
    });
  }

  let is_self_op = subject_coords == clip_coords && subject_path_lengths == clip_path_lengths;
  let pre_op_vertices = if is_self_op {
    collect_vertex_set(subject_coords)
  } else {
    collect_vertex_set_multi(subject_coords, clip_coords)
  };

  let op_id: u32 = match op {
    BooleanOp::Union => 0,
    BooleanOp::Intersect => 1,
    BooleanOp::Difference => 2,
    BooleanOp::Xor => 3,
  };

  let ok = cgal_path_boolean_2d(
    subject_coords,
    subject_path_lengths,
    clip_coords,
    clip_path_lengths,
    op_id,
  );
  if !ok {
    let err = cgal_get_last_error().unwrap_or_else(|| "unknown CGAL error".to_owned());
    return Err(ErrorStack::new(format!(
      "`{fn_name}` (cgal engine) failed: {err}"
    )));
  }

  let out_coords = cgal_get_path_boolean_2d_coords();
  let out_lengths = cgal_get_path_boolean_2d_path_lengths();
  cgal_clear_path_boolean_2d_output();

  let mut paths = Vec::with_capacity(out_lengths.len());
  let mut coord_ix = 0usize;
  for len in out_lengths {
    let mut path = Vec::with_capacity(len as usize);
    for _ in 0..len {
      if coord_ix + 1 >= out_coords.len() {
        break;
      }
      path.push(Vec2::new(out_coords[coord_ix], out_coords[coord_ix + 1]));
      coord_ix += 2;
    }
    if path.len() >= 2 {
      paths.push(path);
    }
  }

  let critical_t_values = global_critical_points(&paths, &pre_op_vertices);

  Ok(BooleanResult {
    paths,
    critical_t_values,
  })
}

#[cfg(target_arch = "wasm32")]
fn sample_path_to_coords(
  ctx: &EvalCtx,
  path_callable: &Rc<Callable>,
  curve_angle_radians: f32,
  sample_count: usize,
  closed_override: Option<bool>,
  fn_name: &str,
) -> Result<(Vec<f32>, Vec<u32>), ErrorStack> {
  let subpaths = sample_path_subpaths(
    ctx,
    path_callable,
    curve_angle_radians,
    sample_count,
    closed_override,
    fn_name,
  )?;

  let mut coords = Vec::new();
  let mut lengths = Vec::new();
  for (points, _is_closed) in subpaths {
    lengths.push(points.len() as u32);
    for pt in &points {
      coords.push(pt.x);
      coords.push(pt.y);
    }
  }
  Ok((coords, lengths))
}

#[cfg(target_arch = "wasm32")]
struct BooleanOpts {
  fill_rule: FillRule,
  curve_angle_radians: f32,
  sample_count: usize,
  closed_override: Option<bool>,
  engine: BooleanEngine,
}

/// `opt_refs` are the `fill_rule, curve_angle_degrees, sample_count, closed, engine` arg refs.
#[cfg(target_arch = "wasm32")]
fn parse_boolean_opts(
  ctx: &EvalCtx,
  opt_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  fn_name: &str,
) -> Result<BooleanOpts, ErrorStack> {
  let fill_rule_val = opt_refs[0].resolve(args, kwargs);

  let curve_angle_degrees =
    ctx.resolve_curve_angle_degrees(opt_refs[1].resolve(args, kwargs)) as f64;
  if curve_angle_degrees <= 0.0 {
    return Err(ErrorStack::new(format!(
      "Invalid curve_angle_degrees for `{fn_name}`; expected > 0, found: {curve_angle_degrees}"
    )));
  }
  let curve_angle_radians = (curve_angle_degrees as f32).to_radians();

  let sample_count_val = opt_refs[2].resolve(args, kwargs);
  let sample_count = match sample_count_val.as_int() {
    Some(v) => v,
    None => {
      return Err(ErrorStack::new(format!(
        "Invalid sample_count for `{fn_name}`; expected int, found: {sample_count_val:?}"
      )))
    }
  };
  let sample_count = sample_count.max(2) as usize;

  let closed_override_val = opt_refs[3].resolve(args, kwargs);
  let closed_override = match closed_override_val {
    Value::Bool(b) => Some(*b),
    Value::Nil => None,
    _ => {
      return Err(ErrorStack::new(format!(
        "Invalid closed argument for `{fn_name}`; expected bool or nil, found: \
         {closed_override_val:?}"
      )))
    }
  };

  let engine = parse_engine(opt_refs[4].resolve(args, kwargs), fn_name)?;

  // Engine-specific default fill rule when caller leaves it unset (nil): Clipper2's
  // historical default is NonZero; CGAL's `Polygon_set_2` natively combines subpaths
  // under EvenOdd so we default to that to avoid forcing the user to opt in twice.
  let fill_rule = if matches!(fill_rule_val, Value::Nil) {
    match engine {
      BooleanEngine::Clipper => FillRule::NonZero,
      BooleanEngine::Cgal => FillRule::EvenOdd,
    }
  } else {
    FillRule::parse(fill_rule_val, fn_name)?
  };

  match engine {
    BooleanEngine::Clipper => {
      crate::or_async_dep_bit(crate::DEP_BIT_CLIPPER2);
      if !clipper2_get_is_loaded() {
        return Err(ErrorStack::new_uninitialized_module("clipper2"));
      }
    }
    BooleanEngine::Cgal => {
      crate::or_async_dep_bit(crate::DEP_BIT_CGAL);
      if !cgal_get_is_loaded() {
        return Err(ErrorStack::new_uninitialized_module("cgal"));
      }
      if fill_rule != FillRule::EvenOdd {
        return Err(ErrorStack::new(format!(
          "`{fn_name}` with engine=\"cgal\" only supports fill_rule=\"evenodd\"; got \
           {fill_rule:?}.  Re-run with engine=\"clipper\" for other fill rules."
        )));
      }
    }
  }

  Ok(BooleanOpts {
    fill_rule,
    curve_angle_radians,
    sample_count,
    closed_override,
    engine,
  })
}

#[cfg(target_arch = "wasm32")]
fn sample_boolean_input(
  ctx: &EvalCtx,
  callable: &Rc<Callable>,
  opts: &BooleanOpts,
  fn_name: &str,
) -> Result<(Vec<f32>, Vec<u32>), ErrorStack> {
  sample_path_to_coords(
    ctx,
    callable,
    opts.curve_angle_radians,
    opts.sample_count,
    opts.closed_override,
    fn_name,
  )
}

#[cfg(target_arch = "wasm32")]
fn cached_boolean(
  cache_key: Vec<u32>,
  run: impl FnOnce() -> Result<BooleanResult, ErrorStack>,
) -> Result<BooleanResult, ErrorStack> {
  if let Some(cached) = bool_result_cache::get(&cache_key) {
    return Ok(cached);
  }
  let result = run()?;
  bool_result_cache::insert(cache_key, &result);
  Ok(result)
}

#[cfg(target_arch = "wasm32")]
fn boolean_result_value(ctx: &EvalCtx, result: BooleanResult, fn_name: &str) -> Value {
  let critical_points = Some(result.critical_t_values);
  let draw_cmds = polylines_to_draw_commands(result.paths.into_iter().map(|p| (p, true)));
  let interned_t_kwarg = ctx.interned_symbols.intern("t");
  let mut tracer = PathTracerCallable::new_with_critical_points(
    false,
    false,
    false,
    draw_cmds,
    interned_t_kwarg,
    critical_points,
  );
  // The op's fill rule is already resolved into the output: rings are non-crossing and
  // winding-consistent, so nesting-based evenodd describes the region exactly.  Carrying the
  // winding-dependent input rule forward instead would push downstream tessellation onto the
  // lyon path, which mishandles the collinear touch configurations boolean outputs contain.
  tracer.fill_rule = Some(FillRule::EvenOdd);
  Value::Callable(Rc::new(Callable::Dynamic {
    name: fn_name.to_owned(),
    inner: Box::new(tracer),
  }))
}

#[cfg(target_arch = "wasm32")]
pub fn path_boolean_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  op: BooleanOp,
  fn_name: &str,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let subject_val = arg_refs[0].resolve(args, kwargs);
      let subject_callable = subject_val.as_callable().ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid subject argument for `{fn_name}`; expected Callable, found: {subject_val:?}"
        ))
      })?;

      let clip_val = arg_refs[1].resolve(args, kwargs);
      let clip_callable = clip_val.as_callable().ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid clip argument for `{fn_name}`; expected Callable, found: {clip_val:?}"
        ))
      })?;

      let opts = parse_boolean_opts(ctx, &arg_refs[2..], args, kwargs, fn_name)?;
      let (subject_coords, subject_lengths) =
        sample_boolean_input(ctx, subject_callable, &opts, fn_name)?;
      let (clip_coords, clip_lengths) = sample_boolean_input(ctx, clip_callable, &opts, fn_name)?;

      let op_ix = op as u32;
      let engine_discriminant = match opts.engine {
        BooleanEngine::Clipper => op_ix,
        BooleanEngine::Cgal => 8 + op_ix,
      };
      let fill_rule = opts.fill_rule.to_clipper2_u32();
      let cache_key = bool_result_cache::build_key(
        engine_discriminant,
        fill_rule,
        &subject_coords,
        &subject_lengths,
        &clip_coords,
        &clip_lengths,
      );
      let result = cached_boolean(cache_key, || match opts.engine {
        BooleanEngine::Clipper => Ok(run_clipper_boolean(
          &subject_coords,
          &subject_lengths,
          &clip_coords,
          &clip_lengths,
          fill_rule,
          op,
        )),
        BooleanEngine::Cgal => run_cgal_boolean(
          &subject_coords,
          &subject_lengths,
          &clip_coords,
          &clip_lengths,
          op,
          fn_name,
        ),
      })?;
      Ok(boolean_result_value(ctx, result, fn_name))
    }
    // n-ary union of a whole sequence: one boolean pass instead of a pairwise chain that
    // re-samples and re-analyzes the growing accumulator at every step
    1 => {
      let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      let opts = parse_boolean_opts(ctx, &arg_refs[1..], args, kwargs, fn_name)?;
      let mut inputs: Vec<(Vec<f32>, Vec<u32>)> = Vec::new();
      for (i, res) in seq.consume(ctx).enumerate() {
        let val = res
          .map_err(|err| err.wrap(format!("Error evaluating sequence passed to `{fn_name}`")))?;
        let callable = val.as_callable().ok_or_else(|| {
          ErrorStack::new(format!(
            "Invalid element at index {i} in sequence passed to `{fn_name}`; expected path \
             Callable, found: {val:?}"
          ))
        })?;
        inputs.push(sample_boolean_input(ctx, callable, &opts, fn_name)?);
      }

      let fill_rule = opts.fill_rule.to_clipper2_u32();
      let result = match opts.engine {
        BooleanEngine::Clipper => {
          let mut coords = Vec::new();
          let mut lengths = Vec::new();
          for (c, l) in &inputs {
            coords.extend_from_slice(c);
            lengths.extend_from_slice(l);
          }
          let cache_key = bool_result_cache::build_key(16, fill_rule, &coords, &lengths, &[], &[]);
          cached_boolean(cache_key, || {
            Ok(run_clipper_self_union(&coords, &lengths, fill_rule))
          })?
        }
        // CGAL has no n-ary entry point, so fold pairwise; a lone input still goes through the
        // op so it gets normalized like the clipper path does
        BooleanEngine::Cgal => {
          let mut inputs = inputs.into_iter();
          let (mut acc_coords, mut acc_lengths) = inputs.next().unwrap_or_default();
          let mut result = None;
          for (coords, lengths) in inputs {
            let r = run_cgal_boolean(
              &acc_coords,
              &acc_lengths,
              &coords,
              &lengths,
              BooleanOp::Union,
              fn_name,
            )?;
            (acc_coords, acc_lengths) = paths_to_flat(&r.paths);
            result = Some(r);
          }
          match result {
            Some(r) => r,
            None => run_cgal_boolean(
              &acc_coords,
              &acc_lengths,
              &acc_coords,
              &acc_lengths,
              BooleanOp::Union,
              fn_name,
            )?,
          }
        }
      };
      Ok(boolean_result_value(ctx, result, fn_name))
    }
    _ => unimplemented!(),
  }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path_boolean_impl(
  _ctx: &EvalCtx,
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
  _op: (),
  fn_name: &str,
) -> Result<Value, ErrorStack> {
  Err(ErrorStack::new(format!(
    "`{fn_name}` is only supported in wasm builds"
  )))
}

// Wrapper functions for each operation
#[cfg(target_arch = "wasm32")]
pub fn path_union_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(
    ctx,
    def_ix,
    arg_refs,
    args,
    kwargs,
    BooleanOp::Union,
    "path_union",
  )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path_union_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(ctx, def_ix, arg_refs, args, kwargs, (), "path_union")
}

#[cfg(target_arch = "wasm32")]
pub fn path_intersect_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(
    ctx,
    def_ix,
    arg_refs,
    args,
    kwargs,
    BooleanOp::Intersect,
    "path_intersect",
  )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path_intersect_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(ctx, def_ix, arg_refs, args, kwargs, (), "path_intersect")
}

#[cfg(target_arch = "wasm32")]
pub fn path_difference_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(
    ctx,
    def_ix,
    arg_refs,
    args,
    kwargs,
    BooleanOp::Difference,
    "path_difference",
  )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path_difference_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(ctx, def_ix, arg_refs, args, kwargs, (), "path_difference")
}

#[cfg(target_arch = "wasm32")]
pub fn path_xor_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(
    ctx,
    def_ix,
    arg_refs,
    args,
    kwargs,
    BooleanOp::Xor,
    "path_xor",
  )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path_xor_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  path_boolean_impl(ctx, def_ix, arg_refs, args, kwargs, (), "path_xor")
}

#[cfg(target_arch = "wasm32")]
fn ensure_path_sampler(
  callable: &Rc<Callable>,
  arg_name: &str,
  fn_name: &str,
) -> Result<(), ErrorStack> {
  if as_path_sampler(callable).is_some() {
    return Ok(());
  }
  Err(ErrorStack::new(format!(
    "`{fn_name}` requires `{arg_name}` to be a path sampler with known topology (e.g. from `path \
     {{ ... }}`, `trace_path`, `trace_svg_path`, `text_to_path`, `lerp_path`, `catmull_rom`). \
     Black-box `|t|: vec2` callables are not supported."
  )))
}

#[cfg(target_arch = "wasm32")]
fn coords_aabb(coords: &[f32]) -> Option<(Vec2, Vec2)> {
  if coords.len() < 2 {
    return None;
  }
  let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
  let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
  let mut i = 0;
  while i + 1 < coords.len() {
    let x = coords[i];
    let y = coords[i + 1];
    if x < min.x {
      min.x = x;
    }
    if y < min.y {
      min.y = y;
    }
    if x > max.x {
      max.x = x;
    }
    if y > max.y {
      max.y = y;
    }
    i += 2;
  }
  Some((min, max))
}

#[cfg(target_arch = "wasm32")]
pub fn path_intersects_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      crate::or_async_dep_bit(crate::DEP_BIT_CLIPPER2);
      if !clipper2_get_is_loaded() {
        return Err(ErrorStack::new_uninitialized_module("clipper2"));
      }

      let a_val = arg_refs[0].resolve(args, kwargs);
      let a_callable = a_val.as_callable().ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid `a` argument for `path_intersects`; expected Callable, found: {a_val:?}"
        ))
      })?;

      let b_val = arg_refs[1].resolve(args, kwargs);
      let b_callable = b_val.as_callable().ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid `b` argument for `path_intersects`; expected Callable, found: {b_val:?}"
        ))
      })?;

      ensure_path_sampler(a_callable, "a", "path_intersects")?;
      ensure_path_sampler(b_callable, "b", "path_intersects")?;

      let fill_rule_enum = FillRule::parse(arg_refs[2].resolve(args, kwargs), "path_intersects")?;
      let fill_rule = fill_rule_enum.to_clipper2_u32();

      let curve_angle_degrees =
        ctx.resolve_curve_angle_degrees(arg_refs[3].resolve(args, kwargs)) as f64;
      if curve_angle_degrees <= 0.0 {
        return Err(ErrorStack::new(format!(
          "Invalid curve_angle_degrees for `path_intersects`; expected > 0, found: \
           {curve_angle_degrees}"
        )));
      }
      let curve_angle_radians = (curve_angle_degrees as f32).to_radians();

      let sample_count_val = arg_refs[4].resolve(args, kwargs);
      let sample_count = match sample_count_val.as_int() {
        Some(v) => v.max(2) as usize,
        None => {
          return Err(ErrorStack::new(format!(
            "Invalid sample_count for `path_intersects`; expected int, found: {sample_count_val:?}"
          )))
        }
      };

      let closed_override_val = arg_refs[5].resolve(args, kwargs);
      let closed_override = match closed_override_val {
        Value::Bool(b) => Some(*b),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid closed argument for `path_intersects`; expected bool or nil, found: \
             {closed_override_val:?}"
          )))
        }
      };

      let a_tracer = as_path_tracer(a_callable);
      let b_tracer = as_path_tracer(b_callable);
      if let (Some(a), Some(b)) = (a_tracer, b_tracer) {
        let a_box = a.analytic_aabb().ok().flatten();
        let b_box = b.analytic_aabb().ok().flatten();
        if let (Some((a_min, a_max)), Some((b_min, b_max))) = (a_box, b_box) {
          if a_max.x < b_min.x || b_max.x < a_min.x || a_max.y < b_min.y || b_max.y < a_min.y {
            return Ok(Value::Bool(false));
          }
        }
      }

      let (a_coords, a_lengths) = sample_path_to_coords(
        ctx,
        a_callable,
        curve_angle_radians,
        sample_count,
        closed_override,
        "path_intersects",
      )?;
      let (b_coords, b_lengths) = sample_path_to_coords(
        ctx,
        b_callable,
        curve_angle_radians,
        sample_count,
        closed_override,
        "path_intersects",
      )?;

      if a_coords.is_empty() || b_coords.is_empty() {
        return Ok(Value::Bool(false));
      }

      // Skip the polyline AABB pre-check if both inputs already passed the analytic one —
      // the discretized bound is strictly looser and can't reject anything the exact one didn't.
      if a_tracer.is_none() || b_tracer.is_none() {
        if let (Some((a_min, a_max)), Some((b_min, b_max))) =
          (coords_aabb(&a_coords), coords_aabb(&b_coords))
        {
          if a_max.x < b_min.x || b_max.x < a_min.x || a_max.y < b_min.y || b_max.y < a_min.y {
            return Ok(Value::Bool(false));
          }
        }
      }

      clipper2_boolean_flat(1, fill_rule, &a_coords, &a_lengths, &b_coords, &b_lengths);
      let out_lengths = clipper2_get_output_path_lengths_flat();
      let has_intersection = out_lengths.iter().any(|len| *len > 0);
      clipper2_clear_output_flat();

      Ok(Value::Bool(has_intersection))
    }
    _ => unimplemented!(),
  }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path_intersects_impl(
  _ctx: &EvalCtx,
  def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => Err(ErrorStack::new(
      "`path_intersects` is only supported in wasm builds",
    )),
    _ => unimplemented!(),
  }
}

/// `path_union` over a whole sequence with default options; the `fold`/`reduce` fast path.
pub fn path_union_seq(ctx: &EvalCtx, seq: Rc<dyn Sequence>) -> Result<Value, ErrorStack> {
  let arg_refs = [
    ArgRef::Positional(0),
    ArgRef::Default(Value::Nil),
    ArgRef::Default(Value::Nil),
    ArgRef::Default(Value::Int(64)),
    ArgRef::Default(Value::Nil),
    ArgRef::Default(Value::Nil),
  ];
  path_union_impl(ctx, 1, &arg_refs, &[Value::Sequence(seq)], EMPTY_KWARGS)
}
