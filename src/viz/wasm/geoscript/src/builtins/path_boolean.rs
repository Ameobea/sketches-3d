use fxhash::FxHashMap;

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

#[cfg(target_arch = "wasm32")]
use crate::builtins::trace_path::{
  build_topology_samples, sample_subpath_points, DrawCommand, PathTracerCallable,
};
use crate::{ArgRef, ErrorStack, EvalCtx, Sym, Value};
#[cfg(target_arch = "wasm32")]
use crate::{Callable, Vec2, EMPTY_KWARGS};

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
  fn clipper2_get_output_coords() -> Vec<f64>;
  fn clipper2_get_output_path_lengths() -> Vec<u32>;
  fn clipper2_clear_output();
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
fn parse_fill_rule(value: &Value, fn_name: &str) -> Result<u32, ErrorStack> {
  if let Some(val) = value.as_str() {
    let key = val.to_ascii_lowercase();
    let mapped = match key.as_str() {
      "evenodd" | "even_odd" | "even-odd" => 0,
      "nonzero" | "non_zero" | "non-zero" => 1,
      "positive" => 2,
      "negative" => 3,
      _ => {
        return Err(ErrorStack::new(format!(
          "Invalid fill_rule for `{fn_name}`; expected one of evenodd, nonzero, positive, \
           negative, found: {val}"
        )));
      }
    };
    return Ok(mapped);
  }

  let num = value.as_float().ok_or_else(|| {
    ErrorStack::new(format!(
      "Invalid fill_rule for `{fn_name}`; expected string or number, found: {value:?}"
    ))
  })? as f64;
  if !(0.0..=3.0).contains(&num) {
    return Err(ErrorStack::new(format!(
      "Invalid fill_rule for `{fn_name}`; expected in [0, 3], found: {num}"
    )));
  }
  Ok(num as u32)
}

#[cfg(target_arch = "wasm32")]
fn build_draw_commands(paths: Vec<Vec<Vec2>>) -> Vec<DrawCommand> {
  let mut cmds = Vec::new();
  for mut points in paths {
    if points.len() < 2 {
      continue;
    }

    if let (Some(first), Some(last)) = (points.first(), points.last()) {
      if (*first - *last).norm() <= 1e-6 {
        points.pop();
      }
    }

    let Some(first) = points.first().copied() else {
      continue;
    };
    cmds.push(DrawCommand::MoveTo(first));
    for pt in points.iter().skip(1) {
      cmds.push(DrawCommand::LineTo(*pt));
    }
    cmds.push(DrawCommand::Close);
  }
  cmds
}

#[cfg(target_arch = "wasm32")]
fn run_clipper_boolean(
  subject_coords: &[f64],
  subject_path_lengths: &[u32],
  clip_coords: &[f64],
  clip_path_lengths: &[u32],
  fill_rule: u32,
  op: BooleanOp,
) -> Vec<Vec<Vec2>> {
  if subject_coords.is_empty() || subject_path_lengths.is_empty() {
    return Vec::new();
  }

  match op {
    BooleanOp::Union => clipper2_union_paths(
      subject_coords,
      subject_path_lengths,
      clip_coords,
      clip_path_lengths,
      fill_rule,
    ),
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

  paths
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
  let mut coords = Vec::new();
  let mut lengths = Vec::new();

  let tracer = match &**path_callable {
    Callable::Dynamic { inner, .. } => inner.as_any().downcast_ref::<PathTracerCallable>(),
    _ => None,
  };

  if let Some(tracer) = tracer {
    for subpath in &tracer.subpaths {
      let is_closed = closed_override.unwrap_or(subpath.is_closed());
      let include_end = !is_closed;
      let mut points = sample_subpath_points(subpath, curve_angle_radians, include_end);
      if tracer.reverse {
        points.reverse();
      }
      if points.len() >= 2 {
        lengths.push(points.len() as u32);
        for pt in &points {
          coords.push(pt.x as f64);
          coords.push(pt.y as f64);
        }
      }
    }
  } else {
    let sample_point = |t: f32| -> Result<Vec2, ErrorStack> {
      let out = ctx
        .invoke_callable(path_callable, &[Value::Float(t)], EMPTY_KWARGS)
        .map_err(|err| err.wrap(&format!("Error sampling callable passed to `{fn_name}`")))?;
      let point = out.as_vec2().ok_or_else(|| {
        ErrorStack::new(format!(
          "Expected Vec2 from callable passed to `{fn_name}`, found: {out:?}"
        ))
      })?;
      Ok(*point)
    };

    let is_closed = if let Some(closed) = closed_override {
      closed
    } else {
      let p0 = sample_point(0.0)?;
      let p1 = sample_point(1.0)?;
      (p0 - p1).norm() <= 1e-4
    };

    let t_samples = build_topology_samples(sample_count, None, None, !is_closed);
    let mut points = Vec::with_capacity(t_samples.len());
    for t in t_samples {
      points.push(sample_point(t)?);
    }

    if points.len() >= 2 {
      lengths.push(points.len() as u32);
      for pt in &points {
        coords.push(pt.x as f64);
        coords.push(pt.y as f64);
      }
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
      if !clipper2_get_is_loaded() {
        return Err(ErrorStack::new_uninitialized_module("clipper2"));
      }

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

      let fill_rule = parse_fill_rule(arg_refs[2].resolve(args, kwargs), fn_name)?;

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

      let output_paths = run_clipper_boolean(
        &subject_coords,
        &subject_lengths,
        &clip_coords,
        &clip_lengths,
        fill_rule,
        op,
      );

      let draw_cmds = build_draw_commands(output_paths);

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let tracer = PathTracerCallable::new_with_critical_points(
        false,
        false,
        false,
        draw_cmds,
        interned_t_kwarg,
        None,
      );
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
