use fxhash::FxHashMap;

use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use crate::builtins::path_critical_points::{
  collect_vertex_set, collect_vertex_set_multi, detect_critical_vertices, restore_boundary_anchors,
  CriticalPointConfig, VertexSet,
};
#[cfg(target_arch = "wasm32")]
use crate::builtins::trace_path::{expect_path, sample_path_subpaths, FillRule};
#[cfg(target_arch = "wasm32")]
use crate::path::Path;
#[cfg(target_arch = "wasm32")]
use crate::Vec2;
use crate::{ArgRef, ErrorStack, EvalCtx, Sequence, Sym, Value, EMPTY_KWARGS};

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

  pub fn clear() {
    CACHE.with(|c| c.borrow_mut().clear());
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
    let val_bytes =
      polylines_bytes(&result.paths) + result.anchors.iter().map(Vec::len).sum::<usize>();
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
  anchors: Vec<Vec<bool>>,
}

/// Per-vertex anchor flags for the boolean output: detected corners plus every op-created
/// vertex (one not in `pre_op_vertices`). All boolean outputs are closed.
#[cfg(target_arch = "wasm32")]
fn path_anchors(paths: &[Vec<Vec2>], pre_op_vertices: &VertexSet) -> Vec<Vec<bool>> {
  let config = CriticalPointConfig::default();
  paths
    .iter()
    .map(|p| detect_critical_vertices(p, &config, Some(pre_op_vertices)))
    .collect()
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
      anchors: Vec::new(),
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
      anchors: Vec::new(),
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
  let anchors = path_anchors(&paths, &pre_op_vertices);
  BooleanResult { paths, anchors }
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
      anchors: Vec::new(),
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

  let anchors = path_anchors(&paths, &pre_op_vertices);
  Ok(BooleanResult { paths, anchors })
}

#[cfg(target_arch = "wasm32")]
fn sample_path_to_coords(
  ctx: &EvalCtx,
  path: &Path,
  curve_angle_radians: f32,
  sample_count: usize,
  closed_override: Option<bool>,
) -> Result<(Vec<f32>, Vec<u32>), ErrorStack> {
  let subpaths = sample_path_subpaths(
    ctx,
    path,
    curve_angle_radians,
    sample_count,
    closed_override,
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

  let engine = parse_engine(opt_refs[3].resolve(args, kwargs), fn_name)?;

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
    engine,
  })
}

#[cfg(target_arch = "wasm32")]
fn sample_boolean_input(
  ctx: &EvalCtx,
  path: &Path,
  opts: &BooleanOpts,
  source_anchors: &mut Vec<Vec2>,
) -> Result<(Vec<f32>, Vec<u32>), ErrorStack> {
  let subpaths = path.sample_subpaths_tagged(
    opts.curve_angle_radians,
    f32::INFINITY,
    opts.sample_count,
    ctx,
  )?;
  let mut coords = Vec::new();
  let mut lengths = Vec::new();
  for s in subpaths.into_iter().filter(|s| s.points.len() >= 2) {
    lengths.push(s.points.len() as u32);
    for (p, anchor) in s.points.into_iter().zip(s.anchors) {
      coords.extend([p.x, p.y]);
      if anchor {
        source_anchors.push(p);
      }
    }
  }
  Ok((coords, lengths))
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
fn boolean_result_value(mut result: BooleanResult, source_anchors: &[Vec2]) -> Value {
  restore_boundary_anchors(&mut result.paths, &mut result.anchors, source_anchors);
  // The op's fill rule is already resolved into the output: rings are non-crossing and
  // winding-consistent, so nesting-based evenodd describes the region exactly.  Carrying the
  // winding-dependent input rule forward instead would push downstream tessellation onto the
  // lyon path, which mishandles the collinear touch configurations boolean outputs contain.
  let polylines = result.paths.into_iter().map(|p| (p, true)).collect();
  Value::Path(Rc::new(
    Path::from_polylines(polylines, Some(result.anchors)).with_fill_rule(Some(FillRule::EvenOdd)),
  ))
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
      let subject = expect_path(arg_refs[0].resolve(args, kwargs), fn_name)?;
      let clip = expect_path(arg_refs[1].resolve(args, kwargs), fn_name)?;

      let opts = parse_boolean_opts(ctx, &arg_refs[2..], args, kwargs, fn_name)?;
      let mut source_anchors = Vec::new();
      let (subject_coords, subject_lengths) =
        sample_boolean_input(ctx, subject, &opts, &mut source_anchors)?;
      let (clip_coords, clip_lengths) = if Rc::ptr_eq(subject, clip) {
        (subject_coords.clone(), subject_lengths.clone())
      } else {
        sample_boolean_input(ctx, clip, &opts, &mut source_anchors)?
      };

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
      Ok(boolean_result_value(result, &source_anchors))
    }
    // n-ary union of a whole sequence: one boolean pass instead of a pairwise chain that
    // re-samples and re-analyzes the growing accumulator at every step
    1 => {
      let seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
      let opts = parse_boolean_opts(ctx, &arg_refs[1..], args, kwargs, fn_name)?;
      let mut inputs: Vec<(Vec<f32>, Vec<u32>)> = Vec::new();
      let mut source_anchors = Vec::new();
      for (i, res) in seq.consume(ctx).enumerate() {
        let val = res
          .map_err(|err| err.wrap(format!("Error evaluating sequence passed to `{fn_name}`")))?;
        let path = val.as_path().ok_or_else(|| {
          ErrorStack::new(format!(
            "Invalid element at index {i} in sequence passed to `{fn_name}`; expected a path, \
             found: {val:?}"
          ))
        })?;
        inputs.push(sample_boolean_input(ctx, path, &opts, &mut source_anchors)?);
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
          // Vertices created in an early fold are still operation-created in the final
          // n-ary result, even if the final pairwise pass sees them in its accumulator.
          let pre_op_vertices: VertexSet = inputs
            .iter()
            .flat_map(|(coords, _)| collect_vertex_set(coords))
            .collect();
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
          let mut result = match result {
            Some(r) => r,
            None => run_cgal_boolean(
              &acc_coords,
              &acc_lengths,
              &acc_coords,
              &acc_lengths,
              BooleanOp::Union,
              fn_name,
            )?,
          };
          result.anchors = path_anchors(&result.paths, &pre_op_vertices);
          result
        }
      };
      Ok(boolean_result_value(result, &source_anchors))
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

      let a_path = expect_path(arg_refs[0].resolve(args, kwargs), "path_intersects")?;
      let b_path = expect_path(arg_refs[1].resolve(args, kwargs), "path_intersects")?;
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

      let (a_box, b_box) = (a_path.concrete_aabb(), b_path.concrete_aabb());
      if let (Some((a_min, a_max)), Some((b_min, b_max))) = (a_box, b_box) {
        if a_max.x < b_min.x || b_max.x < a_min.x || a_max.y < b_min.y || b_max.y < a_min.y {
          return Ok(Value::Bool(false));
        }
      }

      let (a_coords, a_lengths) =
        sample_path_to_coords(ctx, a_path, curve_angle_radians, sample_count, None)?;
      let (b_coords, b_lengths) =
        sample_path_to_coords(ctx, b_path, curve_angle_radians, sample_count, None)?;

      if a_coords.is_empty() || b_coords.is_empty() {
        return Ok(Value::Bool(false));
      }

      // Skip the polyline AABB pre-check if both inputs already passed the analytic one —
      // the discretized bound is strictly looser and can't reject anything the exact one didn't.
      if a_box.is_none() || b_box.is_none() {
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
  ];
  path_union_impl(ctx, 1, &arg_refs, &[Value::Sequence(seq)], EMPTY_KWARGS)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn clear_memo() {
  bool_result_cache::clear();
}
