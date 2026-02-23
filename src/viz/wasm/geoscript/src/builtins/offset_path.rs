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
  fn clipper2_offset_paths(
    coords: &[f64],
    path_lengths: &[u32],
    path_is_closed: &[u8],
    delta: f64,
    join_type: u32,
    end_type: u32,
    miter_limit: f64,
    arc_tolerance: f64,
    preserve_collinear: bool,
    reverse_solution: bool,
    step_count: u32,
    superellipse_exponent: f64,
    end_extension_scale: f64,
    arrow_back_sweep: f64,
    teardrop_pinch: f64,
    join_angle_threshold: f64,
    chebyshev_spacing: bool,
    simplify_epsilon: f64,
  );
  fn clipper2_get_output_coords() -> Vec<f64>;
  fn clipper2_get_output_path_lengths() -> Vec<u32>;
  fn clipper2_get_output_critical_t_values() -> Vec<f64>;
  fn clipper2_clear_output();
}

#[cfg(target_arch = "wasm32")]
struct OffsetOptions {
  delta: f64,
  join_type: u32,
  end_type: u32,
  miter_limit: f64,
  arc_tolerance: f64,
  preserve_collinear: bool,
  reverse_solution: bool,
  step_count: u32,
  superellipse_exponent: f64,
  end_extension_scale: f64,
  arrow_back_sweep: f64,
  teardrop_pinch: f64,
  join_angle_threshold: f64,
  chebyshev_spacing: bool,
  simplify_epsilon: f64,
}

#[cfg(target_arch = "wasm32")]
struct OffsetResult {
  paths: Vec<Vec<Vec2>>,
  critical_t_values: Vec<f32>,
}

#[cfg(target_arch = "wasm32")]
fn parse_join_type(value: &Value) -> Result<u32, ErrorStack> {
  if let Some(val) = value.as_str() {
    let key = val.to_ascii_lowercase();
    let mapped = match key.as_str() {
      "square" => 0,
      "bevel" => 1,
      "round" => 2,
      "miter" | "mitre" => 3,
      "superellipse" => 4,
      "knob" => 5,
      "step" => 6,
      "spike" => 7,
      _ => {
        // TODO: maybe should have Rust-side enums matching the JS/C++ side so that we can better
        // enumerate and match
        return Err(ErrorStack::new(format!(
          "Invalid join_type for `offset_path`; expected one of square, bevel, round, miter, \
           superellipse, knob, step, spike, found: {val}"
        )));
      }
    };
    return Ok(mapped);
  }

  let num = value.as_float().ok_or_else(|| {
    ErrorStack::new(format!(
      "Invalid join_type for `offset_path`; expected string or number, found: {value:?}"
    ))
  })? as f64;
  if !(0.0..=7.0).contains(&num) {
    return Err(ErrorStack::new(format!(
      "Invalid join_type for `offset_path`; expected in [0, 7], found: {num}"
    )));
  }
  Ok(num as u32)
}

#[cfg(target_arch = "wasm32")]
fn parse_end_type(value: &Value) -> Result<u32, ErrorStack> {
  if let Some(val) = value.as_str() {
    let key = val.to_ascii_lowercase();
    let mapped = match key.as_str() {
      "polygon" => 0,
      "joined" => 1,
      "butt" => 2,
      "square" => 3,
      "round" => 4,
      "superellipse" => 5,
      "triangle" => 6,
      "arrow" => 7,
      "teardrop" => 8,
      _ => {
        return Err(ErrorStack::new(format!(
          "Invalid end_type for `offset_path`; expected one of polygon, joined, butt, square, \
           round, superellipse, triangle, arrow, teardrop, found: {val}"
        )))
      }
    };
    return Ok(mapped);
  }

  let num = value.as_float().ok_or_else(|| {
    ErrorStack::new(format!(
      "Invalid end_type for `offset_path`; expected string or number, found: {value:?}"
    ))
  })? as f64;
  if !(0.0..=8.0).contains(&num) {
    return Err(ErrorStack::new(format!(
      "Invalid end_type for `offset_path`; expected in [0, 8], found: {num}"
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
fn run_clipper_offset(
  coords: &[f64],
  path_lengths: &[u32],
  path_is_closed: &[u8],
  opts: &OffsetOptions,
) -> Result<OffsetResult, ErrorStack> {
  if coords.is_empty() || path_lengths.is_empty() {
    return Ok(OffsetResult {
      paths: Vec::new(),
      critical_t_values: Vec::new(),
    });
  }

  clipper2_offset_paths(
    coords,
    path_lengths,
    path_is_closed,
    opts.delta,
    opts.join_type,
    opts.end_type,
    opts.miter_limit,
    opts.arc_tolerance,
    opts.preserve_collinear,
    opts.reverse_solution,
    opts.step_count,
    opts.superellipse_exponent,
    opts.end_extension_scale,
    opts.arrow_back_sweep,
    opts.teardrop_pinch,
    opts.join_angle_threshold,
    opts.chebyshev_spacing,
    opts.simplify_epsilon,
  );

  let out_coords = clipper2_get_output_coords();
  let out_lengths = clipper2_get_output_path_lengths();
  let critical_t_values = clipper2_get_output_critical_t_values();
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

  let critical_t_values = critical_t_values.into_iter().map(|t| t as f32).collect();

  Ok(OffsetResult {
    paths,
    critical_t_values,
  })
}

#[cfg(target_arch = "wasm32")]
pub fn offset_path_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      if !clipper2_get_is_loaded() {
        return Err(ErrorStack::new_uninitialized_module("clipper2"));
      }

      let path_val = arg_refs[0].resolve(args, kwargs);
      let path_callable = path_val.as_callable().ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid path argument for `offset_path`; expected Callable, found: {path_val:?}"
        ))
      })?;

      let delta_val = arg_refs[1].resolve(args, kwargs);
      let delta = delta_val.as_float().ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid delta argument for `offset_path`; expected number, found: {delta_val:?}"
        ))
      })? as f64;

      let join_type = parse_join_type(arg_refs[2].resolve(args, kwargs))?;
      let end_type = parse_end_type(arg_refs[3].resolve(args, kwargs))?;
      let miter_limit = arg_refs[4].resolve(args, kwargs).as_float().unwrap() as f64;
      let arc_tolerance = arg_refs[5].resolve(args, kwargs).as_float().unwrap() as f64;
      let preserve_collinear = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
      let reverse_solution = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();
      let step_count = arg_refs[8].resolve(args, kwargs).as_int().unwrap();
      let step_count = step_count.max(0) as u32;
      let superellipse_exponent = arg_refs[9].resolve(args, kwargs).as_float().unwrap() as f64;
      let end_extension_scale = arg_refs[10].resolve(args, kwargs).as_float().unwrap() as f64;
      let arrow_back_sweep = arg_refs[11].resolve(args, kwargs).as_float().unwrap() as f64;
      let teardrop_pinch = arg_refs[12].resolve(args, kwargs).as_float().unwrap() as f64;
      let join_angle_threshold = arg_refs[13].resolve(args, kwargs).as_float().unwrap() as f64;
      let chebyshev_spacing = arg_refs[14].resolve(args, kwargs).as_bool().unwrap();
      let simplify_epsilon = arg_refs[15].resolve(args, kwargs).as_float().unwrap() as f64;
      let curve_angle_degrees = arg_refs[16].resolve(args, kwargs).as_float().unwrap() as f64;
      if curve_angle_degrees <= 0.0 {
        return Err(ErrorStack::new(format!(
          "Invalid curve_angle_degrees for `offset_path`; expected > 0, found: \
           {curve_angle_degrees}"
        )));
      }
      let curve_angle_radians = (curve_angle_degrees as f32).to_radians();
      let sample_count_val = arg_refs[17].resolve(args, kwargs);
      let sample_count = match sample_count_val.as_int() {
        Some(v) => v,
        None => {
          return Err(ErrorStack::new(format!(
            "Invalid sample_count for `offset_path`; expected int, found: {sample_count_val:?}"
          )))
        }
      };
      let sample_count = sample_count.max(2) as usize;
      let closed_override_val = arg_refs[18].resolve(args, kwargs);
      let closed_override = match closed_override_val {
        Value::Bool(b) => Some(*b),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid closed argument for `offset_path`; expected bool or nil, found: \
             {closed_override_val:?}"
          )))
        }
      };

      let opts = OffsetOptions {
        delta,
        join_type,
        end_type,
        miter_limit,
        arc_tolerance,
        preserve_collinear,
        reverse_solution,
        step_count,
        superellipse_exponent,
        end_extension_scale,
        arrow_back_sweep,
        teardrop_pinch,
        join_angle_threshold,
        chebyshev_spacing,
        simplify_epsilon,
      };

      let mut closed_inputs = Vec::new();
      let mut open_inputs = Vec::new();

      let mut push_path = |points: Vec<Vec2>, is_closed: bool| {
        if points.len() < 2 {
          return;
        }
        if is_closed {
          closed_inputs.push(points);
        } else {
          open_inputs.push(points);
        }
      };

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
          push_path(points, is_closed);
        }
      } else {
        let sample_point = |t: f32| -> Result<Vec2, ErrorStack> {
          let out = ctx
            .invoke_callable(path_callable, &[Value::Float(t)], EMPTY_KWARGS)
            .map_err(|err| err.wrap("Error sampling callable passed to `offset_path`"))?;
          let point = out.as_vec2().ok_or_else(|| {
            ErrorStack::new(format!(
              "Expected Vec2 from callable passed to `offset_path`, found: {out:?}"
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

        push_path(points, is_closed);
      }

      let mut output_paths = Vec::new();
      let mut critical_points: Option<Vec<f32>> = None;

      let mut run_group = |paths: &[Vec<Vec2>], is_closed: bool| -> Result<(), ErrorStack> {
        if paths.is_empty() {
          return Ok(());
        }
        let mut coords = Vec::new();
        let mut lengths = Vec::new();
        let mut closed_flags = Vec::new();
        for path in paths {
          if path.len() < 2 {
            continue;
          }
          lengths.push(path.len() as u32);
          closed_flags.push(if is_closed { 1 } else { 0 });
          for pt in path {
            coords.push(pt.x as f64);
            coords.push(pt.y as f64);
          }
        }

        let result = run_clipper_offset(&coords, &lengths, &closed_flags, &opts)?;
        if output_paths.is_empty() && result.paths.len() == 1
        // with new critical t detection logic in the Clipper2 fork, a case of 0 critical points is
        // valid.  Using the adaptive sampler handles distributing points more intelligently along
        // the path perimeter so we can force the inclusion of 0 explicit critical t values here.
        //
        // && !result.critical_t_values.is_empty()
        {
          critical_points = Some(result.critical_t_values.clone());
        }
        output_paths.extend(result.paths);
        Ok(())
      };

      run_group(&closed_inputs, true)?;
      run_group(&open_inputs, false)?;

      let output_path_count = output_paths.len();
      let draw_cmds = build_draw_commands(output_paths);
      if output_path_count != 1 {
        critical_points = None;
      }

      // TODO: should have a `PathTracerCallable::new()` to avoid leaking this internal detail and
      // to simplify things
      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let tracer = PathTracerCallable::new_with_critical_points(
        false,
        false,
        false,
        draw_cmds,
        interned_t_kwarg,
        critical_points,
      );
      return Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "offset_path".to_owned(),
        inner: Box::new(tracer),
      })));
    }
    _ => unimplemented!(),
  }
}

#[cfg(not(target_arch = "wasm32"))]
pub fn offset_path_impl(
  _ctx: &EvalCtx,
  def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => Err(ErrorStack::new(
      "`offset_path` is only supported in wasm builds",
    )),
    _ => unimplemented!(),
  }
}
