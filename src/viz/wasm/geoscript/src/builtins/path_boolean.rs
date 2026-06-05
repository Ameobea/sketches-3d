use fxhash::FxHashMap;

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use crate::builtins::path_critical_points::{
  collect_vertex_set, collect_vertex_set_multi, detect_critical_points, CriticalPointConfig,
};
#[cfg(target_arch = "wasm32")]
use crate::builtins::trace_path::{
  as_path_sampler, as_path_tracer, polylines_to_draw_commands, sample_path_subpaths, FillRule,
  PathTracerCallable,
};
use crate::{ArgRef, ErrorStack, EvalCtx, Sym, Value};
#[cfg(target_arch = "wasm32")]
use crate::{Callable, Vec2};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasm/clipper2/clipper2")]
extern "C" {
  fn clipper2_get_is_loaded() -> bool;
  fn clipper2_union_paths(
    subject_coords: &[f64],
    subject_path_lengths: &[u32],
    clip_coords: &[f64],
    clip_path_lengths: &[u32],
    fill_rule: u32,
  );
  fn clipper2_intersect_paths(
    subject_coords: &[f64],
    subject_path_lengths: &[u32],
    clip_coords: &[f64],
    clip_path_lengths: &[u32],
    fill_rule: u32,
  );
  fn clipper2_difference_paths(
    subject_coords: &[f64],
    subject_path_lengths: &[u32],
    clip_coords: &[f64],
    clip_path_lengths: &[u32],
    fill_rule: u32,
  );
  fn clipper2_xor_paths(
    subject_coords: &[f64],
    subject_path_lengths: &[u32],
    clip_coords: &[f64],
    clip_path_lengths: &[u32],
    fill_rule: u32,
  );
  fn clipper2_union_self(subject_coords: &[f64], subject_path_lengths: &[u32], fill_rule: u32);
  fn clipper2_get_output_coords() -> Vec<f64>;
  fn clipper2_get_output_path_lengths() -> Vec<u32>;
  fn clipper2_clear_output();
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
struct BooleanResult {
  paths: Vec<Vec<Vec2>>,
  critical_t_values: Vec<f32>,
}

#[cfg(target_arch = "wasm32")]
fn run_clipper_boolean(
  subject_coords: &[f64],
  subject_path_lengths: &[u32],
  clip_coords: &[f64],
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

  let is_self_union = op == BooleanOp::Union
    && subject_coords == clip_coords
    && subject_path_lengths == clip_path_lengths;
  let pre_op_vertices = if is_self_union {
    collect_vertex_set(subject_coords)
  } else {
    collect_vertex_set_multi(subject_coords, clip_coords)
  };

  match op {
    BooleanOp::Union => {
      if is_self_union {
        clipper2_union_self(subject_coords, subject_path_lengths, fill_rule);
      } else {
        clipper2_union_paths(
          subject_coords,
          subject_path_lengths,
          clip_coords,
          clip_path_lengths,
          fill_rule,
        )
      }
    }
    BooleanOp::Intersect => clipper2_intersect_paths(
      subject_coords,
      subject_path_lengths,
      clip_coords,
      clip_path_lengths,
      fill_rule,
    ),
    BooleanOp::Difference => clipper2_difference_paths(
      subject_coords,
      subject_path_lengths,
      clip_coords,
      clip_path_lengths,
      fill_rule,
    ),
    BooleanOp::Xor => clipper2_xor_paths(
      subject_coords,
      subject_path_lengths,
      clip_coords,
      clip_path_lengths,
      fill_rule,
    ),
  }

  let out_coords = clipper2_get_output_coords();
  let out_lengths = clipper2_get_output_path_lengths();
  clipper2_clear_output();

  let mut paths = Vec::with_capacity(out_lengths.len());
  let mut coord_ix = 0usize;
  for len in out_lengths {
    let mut path = Vec::with_capacity(len as usize);
    for _ in 0..len {
      if coord_ix + 1 >= out_coords.len() {
        break;
      }
      let x = out_coords[coord_ix];
      let y = out_coords[coord_ix + 1];
      path.push(Vec2::new(x as f32, y as f32));
      coord_ix += 2;
    }
    if path.len() >= 2 {
      paths.push(path);
    }
  }

  let critical_t_values = detect_critical_points(
    &paths,
    &CriticalPointConfig::default(),
    Some(&pre_op_vertices),
  );

  BooleanResult {
    paths,
    critical_t_values,
  }
}

#[cfg(target_arch = "wasm32")]
fn run_cgal_boolean(
  subject_coords: &[f64],
  subject_path_lengths: &[u32],
  clip_coords: &[f64],
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

  let subj_f32: Vec<f32> = subject_coords.iter().map(|&v| v as f32).collect();
  let clip_f32: Vec<f32> = clip_coords.iter().map(|&v| v as f32).collect();

  let op_id: u32 = match op {
    BooleanOp::Union => 0,
    BooleanOp::Intersect => 1,
    BooleanOp::Difference => 2,
    BooleanOp::Xor => 3,
  };

  let ok = cgal_path_boolean_2d(
    &subj_f32,
    subject_path_lengths,
    &clip_f32,
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

  // detect_critical_points takes f64 vertex coordinates; the pre-op set was
  // collected from the f64 sample buffer so the comparison is apples-to-apples.
  let critical_t_values = detect_critical_points(
    &paths,
    &CriticalPointConfig::default(),
    Some(&pre_op_vertices),
  );

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
) -> Result<(Vec<f64>, Vec<u32>), ErrorStack> {
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
      coords.push(pt.x as f64);
      coords.push(pt.y as f64);
    }
  }
  Ok((coords, lengths))
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

      let fill_rule_val = arg_refs[2].resolve(args, kwargs);

      let curve_angle_degrees = arg_refs[3].resolve(args, kwargs).as_float().unwrap() as f64;
      if curve_angle_degrees <= 0.0 {
        return Err(ErrorStack::new(format!(
          "Invalid curve_angle_degrees for `{fn_name}`; expected > 0, found: {curve_angle_degrees}"
        )));
      }
      let curve_angle_radians = (curve_angle_degrees as f32).to_radians();

      let sample_count_val = arg_refs[4].resolve(args, kwargs);
      let sample_count = match sample_count_val.as_int() {
        Some(v) => v,
        None => {
          return Err(ErrorStack::new(format!(
            "Invalid sample_count for `{fn_name}`; expected int, found: {sample_count_val:?}"
          )))
        }
      };
      let sample_count = sample_count.max(2) as usize;

      let closed_override_val = arg_refs[5].resolve(args, kwargs);
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

      let engine = parse_engine(arg_refs[6].resolve(args, kwargs), fn_name)?;

      // Engine-specific default fill rule when caller leaves it unset (nil): Clipper2's
      // historical default is NonZero; CGAL's `Polygon_set_2` natively combines subpaths
      // under EvenOdd so we default to that to avoid forcing the user to opt in twice.
      let fill_rule_enum = if matches!(fill_rule_val, Value::Nil) {
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
          if fill_rule_enum != FillRule::EvenOdd {
            return Err(ErrorStack::new(format!(
              "`{fn_name}` with engine=\"cgal\" only supports fill_rule=\"evenodd\"; got \
               {fill_rule_enum:?}.  Re-run with engine=\"clipper\" for other fill rules."
            )));
          }
        }
      }

      let (subject_coords, subject_lengths) = sample_path_to_coords(
        ctx,
        subject_callable,
        curve_angle_radians,
        sample_count,
        closed_override,
        fn_name,
      )?;

      let (clip_coords, clip_lengths) = sample_path_to_coords(
        ctx,
        clip_callable,
        curve_angle_radians,
        sample_count,
        closed_override,
        fn_name,
      )?;

      let boolean_result = match engine {
        BooleanEngine::Clipper => run_clipper_boolean(
          &subject_coords,
          &subject_lengths,
          &clip_coords,
          &clip_lengths,
          fill_rule_enum.to_clipper2_u32(),
          op,
        ),
        BooleanEngine::Cgal => run_cgal_boolean(
          &subject_coords,
          &subject_lengths,
          &clip_coords,
          &clip_lengths,
          op,
          fn_name,
        )?,
      };

      let critical_points = if boolean_result.paths.len() == 1 {
        Some(boolean_result.critical_t_values)
      } else {
        None
      };

      let draw_cmds =
        polylines_to_draw_commands(boolean_result.paths.into_iter().map(|p| (p, true)));

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let mut tracer = PathTracerCallable::new_with_critical_points(
        false,
        false,
        false,
        draw_cmds,
        interned_t_kwarg,
        critical_points,
      );
      // The output has been resolved by the chosen engine using this fill rule, so carry it
      // forward.
      tracer.fill_rule = Some(fill_rule_enum);
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: fn_name.to_owned(),
        inner: Box::new(tracer),
      })))
    }
    _ => unimplemented!(),
  }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn path_boolean_impl(
  _ctx: &EvalCtx,
  def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
  _op: (),
  fn_name: &str,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => Err(ErrorStack::new(format!(
      "`{fn_name}` is only supported in wasm builds"
    ))),
    _ => unimplemented!(),
  }
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
fn coords_aabb(coords: &[f64]) -> Option<(Vec2, Vec2)> {
  if coords.len() < 2 {
    return None;
  }
  let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
  let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
  let mut i = 0;
  while i + 1 < coords.len() {
    let x = coords[i] as f32;
    let y = coords[i + 1] as f32;
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

      let curve_angle_degrees = arg_refs[3].resolve(args, kwargs).as_float().unwrap() as f64;
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

      clipper2_intersect_paths(&a_coords, &a_lengths, &b_coords, &b_lengths, fill_rule);
      let out_lengths = clipper2_get_output_path_lengths();
      let has_intersection = out_lengths.iter().any(|len| *len > 0);
      clipper2_clear_output();

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
