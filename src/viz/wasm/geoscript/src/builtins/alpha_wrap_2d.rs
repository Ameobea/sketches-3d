#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

use fxhash::FxHashMap;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
use crate::builtins::offset_path::global_critical_points;
#[cfg(target_arch = "wasm32")]
use crate::builtins::trace_path::{
  polylines_to_draw_commands, sample_path_subpaths, FillRule, PathTracerCallable,
};
#[cfg(target_arch = "wasm32")]
use crate::mesh_ops::mesh_ops::verify_cgal_loaded;
#[cfg(target_arch = "wasm32")]
use crate::Callable;
#[cfg(target_arch = "wasm32")]
use crate::Vec2;
use crate::{ArgRef, ErrorStack, EvalCtx, Sym, Value};

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasm/cgal/cgal")]
extern "C" {
  fn cgal_alpha_wrap_2d(
    segments: &[f32],
    relative_alpha: f32,
    relative_offset: f32,
    manifold: bool,
    seeds: &[f32],
  ) -> bool;
  fn cgal_alpha_wrap_2d_points(
    points: &[f32],
    relative_alpha: f32,
    relative_offset: f32,
    manifold: bool,
    seeds: &[f32],
  ) -> bool;
  fn cgal_get_path_boolean_2d_coords() -> Vec<f32>;
  fn cgal_get_path_boolean_2d_path_lengths() -> Vec<u32>;
  fn cgal_clear_path_boolean_2d_output();
  fn cgal_get_last_error() -> Option<String>;
}

#[cfg(target_arch = "wasm32")]
fn consume_vec2s(ctx: &EvalCtx, seq: &Value, what: &str) -> Result<Vec<f32>, ErrorStack> {
  let mut out = Vec::new();
  for res in seq.as_sequence().unwrap().consume(ctx) {
    match res? {
      Value::Vec2(v) => out.extend_from_slice(&[v.x, v.y]),
      val => {
        return Err(ErrorStack::new(format!(
          "Expected Vec2 in {what} passed to `alpha_wrap_2d`, found: {val:?}"
        )))
      }
    }
  }
  Ok(out)
}

#[cfg(target_arch = "wasm32")]
fn subpaths_to_segments(subpaths: Vec<(Vec<Vec2>, bool)>) -> Vec<f32> {
  let mut segments = Vec::new();
  for (points, is_closed) in subpaths {
    let n = points.len();
    let pairs = if is_closed && n >= 3 {
      n
    } else {
      n.saturating_sub(1)
    };
    for i in 0..pairs {
      let (a, b) = (points[i], points[(i + 1) % n]);
      if a != b {
        segments.extend_from_slice(&[a.x, a.y, b.x, b.y]);
      }
    }
  }
  segments
}

#[cfg(target_arch = "wasm32")]
fn wrap_output_to_path(ctx: &EvalCtx) -> Result<Value, ErrorStack> {
  let coords = cgal_get_path_boolean_2d_coords();
  let lengths = cgal_get_path_boolean_2d_path_lengths();
  cgal_clear_path_boolean_2d_output();

  let mut paths = Vec::with_capacity(lengths.len());
  let mut offset = 0;
  for &len in &lengths {
    let len = len as usize;
    paths.push(
      coords[offset * 2..(offset + len) * 2]
        .chunks_exact(2)
        .map(|c| Vec2::new(c[0], c[1]))
        .collect::<Vec<_>>(),
    );
    offset += len;
  }
  if paths.is_empty() {
    return Err(ErrorStack::new(
      "`alpha_wrap_2d` produced an empty result.  If `seeds` were given, each seed must lie \
       strictly inside an enclosed region of the input with room for a disk of radius `alpha` \
       around it.",
    ));
  }

  let critical_points = global_critical_points(&paths);
  let draw_cmds = polylines_to_draw_commands(paths.into_iter().map(|p| (p, true)));
  // Rings are simple and non-crossing (outer CCW, holes CW), so nesting under even-odd
  // describes the wrapped region exactly.
  let tracer = PathTracerCallable::new_with_critical_points(
    false,
    false,
    false,
    draw_cmds,
    ctx.interned_symbols.intern("t"),
    critical_points,
  )
  .with_fill_rule(FillRule::EvenOdd);
  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: "alpha_wrap_2d".to_owned(),
    inner: Box::new(tracer),
  })))
}

#[cfg(target_arch = "wasm32")]
pub fn alpha_wrap_2d_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  verify_cgal_loaded()?;

  let alpha = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
  let offset = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
  let manifold = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
  let seeds = match arg_refs[4].resolve(args, kwargs) {
    Value::Nil => Vec::new(),
    seq => consume_vec2s(ctx, seq, "`seeds`")?,
  };

  let ok = match def_ix {
    0 => {
      let path_val = arg_refs[0].resolve(args, kwargs);
      let path_callable = path_val.as_callable().ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid path argument for `alpha_wrap_2d`; expected Callable, found: {path_val:?}"
        ))
      })?;
      let curve_angle_degrees = ctx.resolve_curve_angle_degrees(arg_refs[5].resolve(args, kwargs));
      if curve_angle_degrees <= 0.0 {
        return Err(ErrorStack::new(format!(
          "Invalid curve_angle_degrees for `alpha_wrap_2d`; expected > 0, found: \
           {curve_angle_degrees}"
        )));
      }
      let sample_count = arg_refs[6].resolve(args, kwargs).as_int().unwrap().max(2) as usize;
      let closed_override = match arg_refs[7].resolve(args, kwargs) {
        Value::Bool(b) => Some(*b),
        Value::Nil => None,
        val => {
          return Err(ErrorStack::new(format!(
            "Invalid closed argument for `alpha_wrap_2d`; expected bool or nil, found: {val:?}"
          )))
        }
      };
      let subpaths = sample_path_subpaths(
        ctx,
        path_callable,
        curve_angle_degrees.to_radians(),
        sample_count,
        closed_override,
        "alpha_wrap_2d",
      )?;
      let segments = subpaths_to_segments(subpaths);
      if segments.is_empty() {
        return Err(ErrorStack::new(
          "`alpha_wrap_2d` path input has no segments after sampling",
        ));
      }
      cgal_alpha_wrap_2d(&segments, alpha, offset, manifold, &seeds)
    }
    1 => {
      let points = consume_vec2s(ctx, arg_refs[0].resolve(args, kwargs), "sequence")?;
      if points.is_empty() {
        return Err(ErrorStack::new("`alpha_wrap_2d` points input is empty"));
      }
      cgal_alpha_wrap_2d_points(&points, alpha, offset, manifold, &seeds)
    }
    _ => unimplemented!(),
  };

  if !ok {
    let err = cgal_get_last_error().unwrap_or_else(|| "unknown CGAL error".to_owned());
    return Err(ErrorStack::new(format!(
      "Error in `alpha_wrap_2d` function: {err}"
    )));
  }
  wrap_output_to_path(ctx)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn alpha_wrap_2d_impl(
  _ctx: &EvalCtx,
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  Err(ErrorStack::new(
    "2D alpha wrapping is not supported outside of wasm",
  ))
}
