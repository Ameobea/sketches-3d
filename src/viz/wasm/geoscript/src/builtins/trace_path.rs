use crate::ValueMap;
use std::any::Any;
use std::cmp::Ordering;
use std::f32::consts::PI;
use std::rc::Rc;

use fxhash::FxHashMap;
use svgtypes::PathParser;

pub(crate) use crate::path::segment::*;
pub(crate) use crate::path::{DrawCommand, FillRule};

use crate::{
  path::Path, ArgRef, ArgType, Callable, DynamicCallable, ErrorStack, EvalCtx, Sym, Value, Vec2,
};

const GUIDE_EPSILON: f32 = 1e-6;

pub(crate) fn normalize_guides(guides: &[f32]) -> Vec<f32> {
  let mut out: Vec<f32> = guides
    .iter()
    .copied()
    .filter(|v| v.is_finite())
    .map(|v| v.clamp(0., 1.))
    .collect();
  out.push(0.);
  out.push(1.);
  out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
  out.dedup_by(|a, b| (*a - *b).abs() <= GUIDE_EPSILON);
  out
}

/// Normalizes a path sampler's critical t-values for use as adaptive sampling guides.
///
/// For a single closed subpath, rotates the parameterization so the earliest critical
/// point aligns to t=0, eliminating the wasted mandatory sample at an arbitrary seam.
///
/// Returns `(normalized_guides, rotation_offset)`. When `rotation_offset > 0`, callers
/// must invoke the underlying sampler at `(t + rotation_offset).rem_euclid(1.0)` to
/// convert from rotated t-space back to the sampler's original t-space.

/// Normalizes a path's critical t-values for use as adaptive sampling guides.
///
/// For a single closed subpath, rotates the parameterization so the earliest critical
/// point aligns to t=0, eliminating the wasted mandatory sample at an arbitrary seam.
///
/// Returns `(normalized_guides, rotation_offset)`. When `rotation_offset > 0`, callers
/// must sample the path at `(t + rotation_offset).rem_euclid(1.0)` to convert from rotated
/// t-space back to the path's original t-space.
pub(crate) fn normalize_path_sampler_guides(path: &Path) -> (Vec<f32>, f32) {
  let raw_cps = path.critical_t_values();
  let is_closed_single = path
    .subpath_topology()
    .map(|t| t.len() == 1 && t[0].closed)
    .unwrap_or(false);

  if !is_closed_single {
    return (normalize_guides(&raw_cps), 0.0);
  }

  let t_min = raw_cps
    .iter()
    .copied()
    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0)
    .fold(f32::INFINITY, f32::min);

  if t_min.is_infinite() || t_min <= GUIDE_EPSILON {
    return (normalize_guides(&raw_cps), 0.0);
  }

  let rotated: Vec<f32> = raw_cps
    .iter()
    .map(|&t| (t - t_min).rem_euclid(1.0))
    .collect();
  (normalize_guides(&rotated), t_min)
}

/// Adapts a `Path` to the callable protocol for machinery that invokes samplers with
/// `(t)` / `(t, ix)` args (rail_sweep profiles). Not user-visible.
pub(crate) struct PathCallable {
  pub path: Rc<Path>,
}

impl DynamicCallable for PathCallable {
  fn as_any(&self) -> &dyn Any {
    self
  }

  fn invoke(
    &self,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let t = match (args.first(), kwargs.len()) {
      (Some(t), 0) => t.as_float(),
      (None, 1) => kwargs
        .get(&ctx.interned_symbols.intern("t"))
        .and_then(Value::as_float),
      _ => None,
    };
    let Some(t) = t else {
      return Err(ErrorStack::new(
        "paths take exactly one numeric argument `t`",
      ));
    };
    Ok(Value::Vec2(self.path.eval_at(t, ctx)?))
  }

  fn get_return_type_hint(&self) -> Option<ArgType> {
    Some(ArgType::Vec2)
  }

  fn is_side_effectful(&self) -> bool {
    false
  }

  fn is_rng_dependent(&self) -> bool {
    false
  }
}

/// A `Path` value as a callable, or the callable itself; errors for anything else.
pub(crate) fn path_or_callable(val: &Value, what: &str) -> Result<Rc<Callable>, ErrorStack> {
  match val {
    Value::Callable(c) => Ok(Rc::clone(c)),
    Value::Path(p) => Ok(Rc::new(Callable::Dynamic {
      name: "path".to_owned(),
      inner: Box::new(PathCallable { path: Rc::clone(p) }),
    })),
    other => Err(ErrorStack::new(format!(
      "Invalid {what}; expected a path or callable, found: {other:?}"
    ))),
  }
}

/// The `Path` behind a `PathCallable` adapter, if that is what this callable is.
pub(crate) fn callable_path(callable: &Callable) -> Option<&Rc<Path>> {
  match callable {
    Callable::Dynamic { inner, .. } => inner
      .as_any()
      .downcast_ref::<PathCallable>()
      .map(|p| &p.path),
    _ => None,
  }
}

pub(crate) fn expect_path<'a>(val: &'a Value, fn_name: &str) -> Result<&'a Rc<Path>, ErrorStack> {
  val
    .as_path()
    .ok_or_else(|| ErrorStack::new(format!("`{fn_name}`: expected a path, found: {val:?}")))
}

fn uniform_samples(count: usize, include_end: bool) -> Vec<f32> {
  if count == 0 {
    return Vec::new();
  }
  if include_end {
    if count == 1 {
      return vec![0.0];
    }
    let denom = (count - 1) as f32;
    return (0..count).map(|i| i as f32 / denom).collect();
  }

  let denom = count as f32;
  (0..count).map(|i| i as f32 / denom).collect()
}

pub(crate) fn build_topology_samples(
  sample_count: usize,
  guides: Option<&[f32]>,
  interval_weights: Option<&[f32]>,
  include_end: bool,
) -> Vec<f32> {
  let Some(guides) = guides else {
    return uniform_samples(sample_count, include_end);
  };

  let guide_points = normalize_guides(guides);

  if guide_points.len() < 2 {
    return uniform_samples(sample_count, include_end);
  }

  let interval_count = guide_points.len() - 1;
  let base_count = if include_end {
    interval_count + 1
  } else {
    interval_count
  };
  let target_count = sample_count.max(base_count);
  let remaining = target_count - base_count;
  if remaining == 0 {
    return if include_end {
      guide_points
    } else {
      guide_points[..guide_points.len() - 1].to_vec()
    };
  }

  let weights = interval_weights.filter(|weights| weights.len() == interval_count);
  let mut spans = Vec::with_capacity(interval_count);
  let mut total_effective = 0.0;
  for (ix, &[start, end]) in guide_points.array_windows::<2>().enumerate() {
    let span = (end - start).max(0.);
    let weight = weights.map(|weights| weights[ix]).unwrap_or(1.).max(0.);
    let effective = span * weight;
    spans.push((span, effective));
    total_effective += effective;
  }

  if total_effective <= GUIDE_EPSILON {
    total_effective = 0.0;
    for (span, effective) in spans.iter_mut() {
      *effective = *span;
      total_effective += *effective;
    }
  }

  let mut allocations = Vec::with_capacity(interval_count);
  let mut remainders: Vec<(f32, usize)> = Vec::with_capacity(interval_count);
  let mut assigned = 0usize;

  for (ix, (span, effective)) in spans.iter().enumerate() {
    if *span <= 0.0 {
      allocations.push(0);
      remainders.push((0.0, ix));
      continue;
    }
    let exact = if total_effective > 0.0 {
      (remaining as f32) * (effective / total_effective)
    } else {
      0.0
    };
    let count = exact.floor() as usize;
    assigned += count;
    allocations.push(count);
    remainders.push((exact - count as f32, ix));
  }

  let mut leftover = remaining.saturating_sub(assigned);
  remainders.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(Ordering::Equal));
  for (_, ix) in remainders {
    if leftover == 0 {
      break;
    }
    allocations[ix] += 1;
    leftover -= 1;
  }

  let mut samples = Vec::with_capacity(target_count);
  for (ix, &[start, end]) in guide_points.array_windows::<2>().enumerate() {
    samples.push(start);

    let count = allocations[ix];
    if count == 0 {
      continue;
    }
    let step = (end - start) / ((count + 1) as f32);
    for j in 1..=count {
      samples.push(start + step * (j as f32));
    }
  }

  if include_end {
    samples.push(*guide_points.last().unwrap_or(&1.));
  }

  samples
}

/// Discretizes a path into per-subpath polylines: adaptive curvature-based sampling driven by
/// `curve_angle_radians`; lazy leaves use `sample_count` initial probes plus known boundaries.
/// `closed_override` replaces every subpath's closedness. Subpaths with fewer than 2 points
/// are dropped.
pub(crate) fn sample_path_subpaths(
  ctx: &EvalCtx,
  path: &Path,
  curve_angle_radians: f32,
  sample_count: usize,
  closed_override: Option<bool>,
) -> Result<Vec<(Vec<Vec2>, bool)>, ErrorStack> {
  let mut out = path.sample_subpaths(curve_angle_radians, sample_count, ctx)?;
  out.retain(|(points, _)| points.len() >= 2);
  if let Some(closed) = closed_override {
    for (_, c) in &mut out {
      *c = closed;
    }
  }
  Ok(out)
}

/// Builds tagged-dict `Value::Map` representations of every segment in a concrete path, in
/// subpath order. Common fields: `type` (line/quad/cubic/arc), `start`, `end`, `length`,
/// `subpath`, `closed`, `t_start`, `t_end` (subpath-local arc-length parameters in [0, 1]),
/// `t_start_global`, `t_end_global` (across the full path). Curve variants add their control
/// points / arc parameters.
pub(crate) fn build_segment_dicts(path: &Path, fn_name: &str) -> Result<Vec<Value>, ErrorStack> {
  if let Some(lazy) = path.first_lazy_name() {
    return Err(ErrorStack::new(format!(
      "`{fn_name}`: path has no draw commands (contains `{lazy}`); use `discretize_path` first"
    )));
  }
  let leaves = path.leaves();
  let global_total: f32 = leaves.iter().map(|sp| sp.total_length()).sum();
  let mut out = Vec::with_capacity(leaves.iter().map(|sp| sp.segments.len()).sum());
  let mut global_offset = 0.;
  for (subpath_ix, sp) in leaves.iter().enumerate() {
    let local_total = sp.total_length();
    for (seg_ix, seg) in sp.segments.iter().enumerate() {
      let local_prev = if seg_ix == 0 {
        0.0
      } else {
        sp.cumulative_lengths[seg_ix - 1]
      };
      let local_curr = sp.cumulative_lengths[seg_ix];
      let frac = |num: f32, den: f32, empty: f32| if den > 0.0 { num / den } else { empty };
      out.push(segment_to_dict(
        seg,
        SegmentMeta {
          subpath_ix,
          closed: sp.closed,
          t_start_local: frac(local_prev, local_total, 0.0),
          t_end_local: frac(local_curr, local_total, 1.0),
          t_start_global: frac(global_offset + local_prev, global_total, 0.0),
          t_end_global: frac(global_offset + local_curr, global_total, 1.0),
        },
      ));
    }
    global_offset += local_total;
  }
  Ok(out)
}

struct SegmentMeta {
  subpath_ix: usize,
  closed: bool,
  t_start_local: f32,
  t_end_local: f32,
  t_start_global: f32,
  t_end_global: f32,
}

fn segment_to_dict(seg: &PathSegment, meta: SegmentMeta) -> Value {
  let length = seg.length();
  let mut entries: Vec<(&str, Value)> = match seg {
    PathSegment::Line { start, end, .. } => vec![
      ("type", Value::String("line".to_owned())),
      ("start", Value::Vec2(*start)),
      ("end", Value::Vec2(*end)),
    ],
    PathSegment::Quadratic {
      start, ctrl, end, ..
    } => vec![
      ("type", Value::String("quad".to_owned())),
      ("start", Value::Vec2(*start)),
      ("ctrl", Value::Vec2(*ctrl)),
      ("end", Value::Vec2(*end)),
    ],
    PathSegment::Cubic {
      start,
      ctrl1,
      ctrl2,
      end,
      ..
    } => vec![
      ("type", Value::String("cubic".to_owned())),
      ("start", Value::Vec2(*start)),
      ("ctrl1", Value::Vec2(*ctrl1)),
      ("ctrl2", Value::Vec2(*ctrl2)),
      ("end", Value::Vec2(*end)),
    ],
    PathSegment::Arc {
      end,
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      ..
    } => vec![
      ("type", Value::String("arc".to_owned())),
      ("start", Value::Vec2(seg.start_point())),
      ("end", Value::Vec2(*end)),
      ("center", Value::Vec2(*center)),
      ("rx", Value::Float(*rx)),
      ("ry", Value::Float(*ry)),
      (
        "x_axis_rotation",
        Value::Float(sin_phi.atan2(*cos_phi).to_degrees()),
      ),
      ("large_arc", Value::Bool(theta_delta.abs() > PI)),
      ("sweep", Value::Bool(*theta_delta > 0.0)),
      ("theta_start", Value::Float(*theta_start)),
      ("theta_delta", Value::Float(*theta_delta)),
    ],
  };

  entries.extend([
    ("length", Value::Float(length)),
    ("subpath", Value::Int(meta.subpath_ix as i64)),
    ("closed", Value::Bool(meta.closed)),
    ("t_start", Value::Float(meta.t_start_local)),
    ("t_end", Value::Float(meta.t_end_local)),
    ("t_start_global", Value::Float(meta.t_start_global)),
    ("t_end_global", Value::Float(meta.t_end_global)),
  ]);

  let mut map = ValueMap::default();
  for (k, v) in entries {
    map.insert(k.to_string(), v);
  }
  Value::Map(Rc::new(map))
}

/// Polyline copy of `path`. Existing anchors are retained; refinement vertices stay smooth.
pub(crate) fn discretize_path(
  ctx: &EvalCtx,
  path: &Path,
  curve_angle_radians: f32,
  sample_count: usize,
  closed_override: Option<bool>,
) -> Result<Path, ErrorStack> {
  let (polylines, anchors) = path
    .sample_subpaths_tagged(curve_angle_radians, f32::INFINITY, sample_count, ctx)?
    .into_iter()
    .filter(|s| s.points.len() >= 2)
    .map(|s| ((s.points, closed_override.unwrap_or(s.closed)), s.anchors))
    .unzip();
  Ok(Path::from_polylines(polylines, Some(anchors)).with_fill_rule(path.fill_rule))
}

pub fn discretize_path_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path = expect_path(arg_refs[0].resolve(args, kwargs), "discretize_path")?;

  let curve_angle_degrees =
    ctx.resolve_curve_angle_degrees(arg_refs[1].resolve(args, kwargs)) as f64;
  if curve_angle_degrees <= 0.0 {
    return Err(ErrorStack::new(format!(
      "Invalid curve_angle_degrees for `discretize_path`; expected > 0, found: \
       {curve_angle_degrees}"
    )));
  }
  let curve_angle_radians = (curve_angle_degrees as f32).to_radians();

  let sample_count_val = arg_refs[2].resolve(args, kwargs);
  let sample_count = sample_count_val.as_int().ok_or_else(|| {
    ErrorStack::new(format!(
      "Invalid sample_count for `discretize_path`; expected int, found: {sample_count_val:?}"
    ))
  })?;
  let sample_count = sample_count.max(2) as usize;

  let out = discretize_path(ctx, path, curve_angle_radians, sample_count, None)?;
  Ok(Value::Path(Rc::new(out.with_fill_rule(path.fill_rule))))
}

fn parse_svg_path_to_draw_commands(svg_path_str: &str) -> Result<Vec<DrawCommand>, ErrorStack> {
  let parser = PathParser::from(svg_path_str);

  let mut draw_cmds = Vec::new();
  let mut current_pos = Vec2::new(0.0, 0.0);
  let mut start_pos = Vec2::new(0.0, 0.0); // For ClosePath

  for segment in parser {
    let segment =
      segment.map_err(|err| ErrorStack::new(format!("invalid SVG path data: {err}",)))?;
    match segment {
      svgtypes::PathSegment::MoveTo { abs, x, y } => {
        let pos = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::MoveTo(pos));
        current_pos = pos;
        start_pos = pos;
      }
      svgtypes::PathSegment::LineTo { abs, x, y } => {
        let pos = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::LineTo(pos));
        current_pos = pos;
      }
      svgtypes::PathSegment::HorizontalLineTo { abs, x } => {
        let pos = if abs {
          Vec2::new(x as f32, current_pos.y)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y)
        };
        draw_cmds.push(DrawCommand::LineTo(pos));
        current_pos = pos;
      }
      svgtypes::PathSegment::VerticalLineTo { abs, y } => {
        let pos = if abs {
          Vec2::new(current_pos.x, y as f32)
        } else {
          Vec2::new(current_pos.x, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::LineTo(pos));
        current_pos = pos;
      }
      svgtypes::PathSegment::CurveTo {
        abs,
        x1,
        y1,
        x2,
        y2,
        x,
        y,
      } => {
        let (ctrl1, ctrl2, to) = if abs {
          (
            Vec2::new(x1 as f32, y1 as f32),
            Vec2::new(x2 as f32, y2 as f32),
            Vec2::new(x as f32, y as f32),
          )
        } else {
          (
            Vec2::new(current_pos.x + x1 as f32, current_pos.y + y1 as f32),
            Vec2::new(current_pos.x + x2 as f32, current_pos.y + y2 as f32),
            Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32),
          )
        };
        draw_cmds.push(DrawCommand::CubicBezier { ctrl1, ctrl2, to });
        current_pos = to;
      }
      svgtypes::PathSegment::SmoothCurveTo { abs, x2, y2, x, y } => {
        let (ctrl2, to) = if abs {
          (
            Vec2::new(x2 as f32, y2 as f32),
            Vec2::new(x as f32, y as f32),
          )
        } else {
          (
            Vec2::new(current_pos.x + x2 as f32, current_pos.y + y2 as f32),
            Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32),
          )
        };
        draw_cmds.push(DrawCommand::SmoothCubicBezier { ctrl2, to });
        current_pos = to;
      }
      svgtypes::PathSegment::Quadratic { abs, x1, y1, x, y } => {
        let (ctrl, to) = if abs {
          (
            Vec2::new(x1 as f32, y1 as f32),
            Vec2::new(x as f32, y as f32),
          )
        } else {
          (
            Vec2::new(current_pos.x + x1 as f32, current_pos.y + y1 as f32),
            Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32),
          )
        };
        draw_cmds.push(DrawCommand::QuadraticBezier { ctrl, to });
        current_pos = to;
      }
      svgtypes::PathSegment::SmoothQuadratic { abs, x, y } => {
        let to = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::SmoothQuadraticBezier { to });
        current_pos = to;
      }
      svgtypes::PathSegment::EllipticalArc {
        abs,
        rx,
        ry,
        x_axis_rotation,
        large_arc,
        sweep,
        x,
        y,
      } => {
        let to = if abs {
          Vec2::new(x as f32, y as f32)
        } else {
          Vec2::new(current_pos.x + x as f32, current_pos.y + y as f32)
        };
        draw_cmds.push(DrawCommand::Arc {
          rx: rx as f32,
          ry: ry as f32,
          x_axis_rotation: x_axis_rotation as f32,
          large_arc,
          sweep,
          to,
        });
        current_pos = to;
      }
      svgtypes::PathSegment::ClosePath { abs: _ } => {
        draw_cmds.push(DrawCommand::Close);
        current_pos = start_pos;
      }
    }
  }

  Ok(draw_cmds)
}

pub fn trace_svg_path_impl(
  _ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let svg_path_str = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
      let draw_cmds = parse_svg_path_to_draw_commands(svg_path_str)
        .map_err(|err| err.wrap("Error while parsing SVG path string"))?;
      Ok(Value::Path(Rc::new(Path::from_draw_commands(
        draw_cmds, false,
      ))))
    }
    _ => unimplemented!(),
  }
}

pub fn text_to_path_impl(
  _ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let text = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
      let font_family = arg_refs[1].resolve(args, kwargs).as_str().unwrap();
      let font_size = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
      let font_weight_val = arg_refs[3].resolve(args, kwargs);
      let font_weight = match font_weight_val {
        Value::Int(i) => {
          if *i < 100 || *i > 900 {
            return Err(ErrorStack::new(format!(
              "Invalid font_weight argument for `text_to_path`; expected value in range [100, \
               900], found: {i}"
            )));
          }
          Some(i.to_string())
        }
        Value::String(s) => Some(s.as_str().to_string()),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid font_weight argument for `text_to_path`; expected Int, String, or Nil, \
             found: {font_weight_val:?}"
          )));
        }
      };
      let font_style_val = arg_refs[4].resolve(args, kwargs);
      let font_style = match font_style_val {
        Value::String(s) => Some(s.as_str().to_string()),
        Value::Nil => None,
        _ => {
          return Err(ErrorStack::new(format!(
            "Invalid font_style argument for `text_to_path`; expected String or Nil, found: \
             {font_style_val:?}"
          )));
        }
      };
      let letter_spacing = match arg_refs[5].resolve(args, kwargs) {
        Value::Float(f) => *f,
        Value::Int(i) => *i as f32,
        Value::Nil => 0.,
        other => {
          return Err(ErrorStack::new(format!(
            "Invalid letter_spacing argument for `text_to_path`; expected Float, Int, or Nil, \
             found: {other:?}"
          )));
        }
      };

      #[cfg(target_arch = "wasm32")]
      crate::or_async_dep_bit(crate::DEP_BIT_TEXT2PATH);
      let svg_path = crate::mesh_ops::mesh_ops::get_cached_svg_path_str(
        &text,
        &font_family,
        font_size,
        font_weight.as_deref().unwrap_or(""),
        font_style.as_deref().unwrap_or(""),
        letter_spacing,
      )?;
      let Some(svg_path) = svg_path else {
        let args = [
          text.to_owned(),
          font_family.to_owned(),
          font_size.to_string(),
          font_weight.unwrap_or_default(),
          font_style.unwrap_or_default(),
          letter_spacing.to_string(),
        ];
        return Err(ErrorStack::new_uninitialized_module_with_args(
          "text_to_path",
          args.into_iter(),
        ));
      };

      let draw_cmds = parse_svg_path_to_draw_commands(&svg_path)
        .map_err(|e| e.wrap("Error parsing SVG path from text_to_path"))?;
      Ok(Value::Path(Rc::new(Path::from_draw_commands(
        draw_cmds, false,
      ))))
    }
    _ => unimplemented!(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    parse_and_eval_program,
    path::builder::{circle_subpath, rect_subpath},
  };
  use nalgebra::{Matrix3, Vector3};

  fn assert_vec2_close(actual: Vec2, expected: Vec2) {
    let diff = (actual - expected).norm();
    assert!(
      diff < 1e-4,
      "Expected {expected:?}, got {actual:?} (diff {diff})"
    );
  }

  fn build(cmds: Vec<DrawCommand>, closed: bool, center: bool, reverse: bool) -> Path {
    let mut path = Path::from_draw_commands(cmds, closed);
    if center {
      let (min, max) = path.concrete_aabb().unwrap();
      path = path.translated(-(min + max) * 0.5);
    }
    if reverse {
      path = path.reversed();
    }
    path
  }

  fn sample(path: &Path, t: f32) -> Vec2 {
    path.eval_at(t, &EvalCtx::default()).unwrap()
  }

  fn global_vec2(ctx: &EvalCtx, name: &str) -> Vec2 {
    *ctx.get_global(name).unwrap().as_vec2().unwrap()
  }

  #[test]
  fn test_path_tracer_line_segments() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 3.0)),
    ];
    let tracer = build(cmds, false, false, false);

    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 0.25), Vec2::new(1.0, 0.0));
    assert_vec2_close(sample(&tracer, 0.75), Vec2::new(1.0, 2.0));
  }

  fn assert_aabb_close(actual: (Vec2, Vec2), expected_min: Vec2, expected_max: Vec2) {
    assert_vec2_close(actual.0, expected_min);
    assert_vec2_close(actual.1, expected_max);
  }

  #[test]
  fn test_quadratic_bezier_aabb_interior_extremum() {
    let (min, max) = quadratic_bezier_aabb(
      Vec2::new(0.0, 0.0),
      Vec2::new(1.0, 2.0),
      Vec2::new(2.0, 0.0),
    );
    assert_vec2_close(min, Vec2::new(0.0, 0.0));
    assert_vec2_close(max, Vec2::new(2.0, 1.0));
  }

  #[test]
  fn test_cubic_bezier_aabb_monotonic() {
    let (min, max) = cubic_bezier_aabb(
      Vec2::new(0.0, 0.0),
      Vec2::new(0.25, 0.25),
      Vec2::new(0.5, 0.5),
      Vec2::new(1.0, 1.0),
    );
    assert_vec2_close(min, Vec2::new(0.0, 0.0));
    assert_vec2_close(max, Vec2::new(1.0, 1.0));
  }

  #[test]
  fn test_arc_aabb_full_circle() {
    use std::f32::consts::TAU;
    let (min, max) = arc_aabb(Vec2::new(0.0, 0.0), 5.0, 5.0, 1.0, 0.0, 0.0, TAU);
    assert_vec2_close(min, Vec2::new(-5.0, -5.0));
    assert_vec2_close(max, Vec2::new(5.0, 5.0));
  }

  #[test]
  fn test_arc_aabb_quarter_circle() {
    use std::f32::consts::FRAC_PI_2;
    let (min, max) = arc_aabb(Vec2::new(0.0, 0.0), 5.0, 5.0, 1.0, 0.0, 0.0, FRAC_PI_2);
    assert_vec2_close(min, Vec2::new(0.0, 0.0));
    assert_vec2_close(max, Vec2::new(5.0, 5.0));
  }

  #[test]
  fn test_path_tracer_analytic_aabb_circle() {
    let tracer = Path::leaf(circle_subpath(Vec2::new(3.0, -2.0), 4.0, false));
    let (min, max) = tracer.concrete_aabb().unwrap();
    assert_aabb_close((min, max), Vec2::new(-1.0, -6.0), Vec2::new(7.0, 2.0));
  }

  #[test]
  fn test_analytic_aabb_under_rotation_of_elliptical_arc() {
    // Half-ellipse rx=2, ry=1 from angle 0 to π; rotating by +π/2 must give [(-1,-2), (0,2)],
    // which a sign error in the rotation extraction would get wrong.
    use std::f32::consts::FRAC_PI_2;
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(2.0, 0.0)),
      DrawCommand::Arc {
        rx: 2.0,
        ry: 1.0,
        x_axis_rotation: 0.0,
        large_arc: false,
        sweep: true,
        to: Vec2::new(-2.0, 0.0),
      },
    ];
    let tracer = build(cmds, false, false, false);
    let (lmin, lmax) = tracer.concrete_aabb().unwrap();
    assert_vec2_close(lmin, Vec2::new(-2.0, 0.0));
    assert_vec2_close(lmax, Vec2::new(2.0, 1.0));

    let (sin_a, cos_a) = FRAC_PI_2.sin_cos();
    let rot = Matrix3::new(cos_a, -sin_a, 0.0, sin_a, cos_a, 0.0, 0.0, 0.0, 1.0);
    let rotated = tracer.transformed(&rot);
    let (rmin, rmax) = rotated.concrete_aabb().unwrap();
    assert_vec2_close(rmin, Vec2::new(-1.0, -2.0));
    assert_vec2_close(rmax, Vec2::new(0.0, 2.0));
  }

  #[test]
  fn test_path_tracer_analytic_aabb_under_translation() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 1.0)),
    ];
    let tracer = build(cmds, false, false, false);
    let translated = Matrix3::new(1.0, 0.0, 10.0, 0.0, 1.0, -5.0, 0.0, 0.0, 1.0);
    let (min, max) = tracer.transformed(&translated).concrete_aabb().unwrap();
    assert_vec2_close(min, Vec2::new(10.0, -5.0));
    assert_vec2_close(max, Vec2::new(12.0, -4.0));
  }

  #[test]
  fn test_path_tracer_critical_t_values() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 1.0)),
    ];
    let guides = build(cmds, false, false, false).critical_t_values();

    assert_eq!(guides.len(), 3);
    assert!((guides[0] - 0.0).abs() < 1e-6);
    assert!((guides[1] - 0.5).abs() < 1e-6);
    assert!((guides[2] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_anchored_polyline_critical_points() {
    let square = vec![
      Vec2::new(0.0, 0.0),
      Vec2::new(1.0, 0.0),
      Vec2::new(1.0, 1.0),
      Vec2::new(0.0, 1.0),
    ];
    let all = Path::from_polylines(vec![(square.clone(), true)], Some(vec![vec![true; 4]]));
    assert_eq!(all.critical_t_values(), vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    let some = Path::from_polylines(
      vec![(square, true)],
      Some(vec![vec![false, true, false, true]]),
    );
    assert_eq!(some.critical_t_values(), vec![0.25, 0.75]);
  }

  #[test]
  fn test_path_tracer_multiple_subpaths_sampling() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::MoveTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(11.0, 0.0)),
    ];
    let tracer = build(cmds, false, false, false);

    assert_vec2_close(sample(&tracer, 0.25), Vec2::new(0.5, 0.0));
    assert_vec2_close(sample(&tracer, 0.75), Vec2::new(10.5, 0.0));
  }

  #[test]
  fn test_path_tracer_subpath_close_uses_local_start() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::Close,
      DrawCommand::MoveTo(Vec2::new(5.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(6.0, 0.0)),
      DrawCommand::Close,
    ];
    let tracer = build(cmds, false, false, false);
    let leaves = tracer.leaves();

    assert_eq!(leaves.len(), 2);
    assert_eq!(leaves[1].segments.len(), 2);
    match &leaves[1].segments[1] {
      PathSegment::Line { start, end, .. } => {
        assert_vec2_close(*start, Vec2::new(6.0, 0.0));
        assert_vec2_close(*end, Vec2::new(5.0, 0.0));
      }
      _ => panic!("Expected closing line segment for subpath close"),
    }
  }

  #[test]
  fn test_sample_subpath_points_curvature_detail() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::QuadraticBezier {
        ctrl: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
    ];
    let tracer = build(cmds, false, false, false);
    let points = tracer.leaves()[0].sample_points(std::f32::consts::FRAC_PI_4, f32::INFINITY, true);
    assert_eq!(points.len(), 3);
    assert_vec2_close(points[0], Vec2::new(0.0, 0.0));
    assert_vec2_close(points[2], Vec2::new(2.0, 0.0));
  }

  /// Corners must be emitted bit-exactly, not re-derived through the arc-length
  /// parameterization: an irrational perimeter (2+sqrt(2)) lands that round-trip ~1 ULP off,
  /// enough to break edge coincidence between adjacent polygons in boolean ops.
  #[test]
  fn test_sample_subpath_points_exact_corners() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(0.0, 1.0)),
      DrawCommand::Close,
    ];
    let tracer = build(cmds, true, false, false);
    let points =
      tracer.leaves()[0].sample_points(std::f32::consts::FRAC_PI_4, f32::INFINITY, false);
    assert_eq!(
      points,
      vec![
        Vec2::new(0.0, 0.0),
        Vec2::new(1.0, 0.0),
        Vec2::new(0.0, 1.0)
      ]
    );
  }

  #[test]
  fn test_build_topology_samples_include_end() {
    let samples = build_topology_samples(3, None, None, true);
    assert_eq!(samples.len(), 3);
    assert!((samples[0] - 0.0).abs() < 1e-6);
    assert!((samples[1] - 0.5).abs() < 1e-6);
    assert!((samples[2] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_path_tracer_centering() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 10.0)),
    ];
    let tracer = build(cmds, false, true, false);

    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(-5.0, -5.0));
    assert_vec2_close(sample(&tracer, 0.5), Vec2::new(5.0, -5.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(5.0, 5.0));
  }

  #[test]
  fn test_path_tracer_quadratic_endpoints() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::QuadraticBezier {
        ctrl: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
    ];
    let tracer = build(cmds, false, false, false);

    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(2.0, 0.0));
  }

  #[test]
  fn test_path_tracer_smooth_cubic_reflection() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::CubicBezier {
        ctrl1: Vec2::new(0.0, 1.0),
        ctrl2: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
      DrawCommand::SmoothCubicBezier {
        ctrl2: Vec2::new(4.0, 2.0),
        to: Vec2::new(5.0, 0.0),
      },
    ];
    let tracer = build(cmds, false, false, false);
    let leaves = tracer.leaves();

    assert_eq!(leaves.len(), 1);
    assert_eq!(leaves[0].segments.len(), 2);
    match &leaves[0].segments[1] {
      PathSegment::Cubic { ctrl1, .. } => {
        assert_vec2_close(*ctrl1, Vec2::new(3.0, -1.0));
      }
      _ => panic!("Expected cubic segment for smooth cubic reflection"),
    }
  }

  #[test]
  fn test_path_tracer_smooth_quadratic_reflection() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::QuadraticBezier {
        ctrl: Vec2::new(1.0, 1.0),
        to: Vec2::new(2.0, 0.0),
      },
      DrawCommand::SmoothQuadraticBezier {
        to: Vec2::new(4.0, 0.0),
      },
    ];
    let tracer = build(cmds, false, false, false);
    let leaves = tracer.leaves();

    assert_eq!(leaves.len(), 1);
    assert_eq!(leaves[0].segments.len(), 2);
    match &leaves[0].segments[1] {
      PathSegment::Quadratic { ctrl, .. } => {
        assert_vec2_close(*ctrl, Vec2::new(3.0, -1.0));
      }
      _ => panic!("Expected quadratic segment for smooth quadratic reflection"),
    }
  }

  #[test]
  fn test_path_tracer_arc_endpoints() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(1.0, 0.0)),
      DrawCommand::Arc {
        rx: 1.0,
        ry: 1.0,
        x_axis_rotation: 0.0,
        large_arc: false,
        sweep: true,
        to: Vec2::new(-1.0, 0.0),
      },
    ];
    let tracer = build(cmds, false, false, false);

    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(1.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(-1.0, 0.0));
  }

  #[test]
  fn test_path_tracer_closed_flag_adds_closing_segment() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 2.0)),
    ];
    let tracer = build(cmds, true, false, false);

    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(0.0, 0.0));
  }

  fn svg(d: &str) -> Path {
    build(
      parse_svg_path_to_draw_commands(d).unwrap(),
      false,
      false,
      false,
    )
  }

  #[test]
  fn test_parse_svg_path_absolute_line() {
    let tracer = svg("M 0 0 L 10 0 L 10 10");
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 0.5), Vec2::new(10.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_relative_line() {
    let tracer = svg("M 5 5 l 10 0 l 0 10");
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(5.0, 5.0));
    assert_vec2_close(sample(&tracer, 0.5), Vec2::new(15.0, 5.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(15.0, 15.0));
  }

  #[test]
  fn test_parse_svg_path_horizontal_vertical() {
    let tracer = svg("M 0 0 H 10 V 10");
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 0.5), Vec2::new(10.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_cubic_bezier() {
    let tracer = svg("M 0 0 C 3 5, 7 5, 10 0");
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_quadratic_bezier() {
    let tracer = svg("M 0 0 Q 5 5, 10 0");
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_arc() {
    let tracer = svg("M 1 0 A 1 1 0 0 1 -1 0");
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(1.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(-1.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_close() {
    let cmds = parse_svg_path_to_draw_commands("M 0 0 L 10 0 L 5 10 Z").unwrap();
    assert_eq!(cmds.len(), 4);
    assert!(matches!(cmds[3], DrawCommand::Close));

    let tracer = build(cmds, false, false, false);
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(0.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_smooth_cubic() {
    let cmds = parse_svg_path_to_draw_commands("M 0 0 C 0 5, 5 5, 5 0 S 10 -5, 10 0").unwrap();
    assert!(matches!(cmds[2], DrawCommand::SmoothCubicBezier { .. }));
    let tracer = build(cmds, false, false, false);

    assert!(matches!(
      tracer.leaves()[0].segments[1],
      PathSegment::Cubic { .. }
    ));
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_smooth_quadratic() {
    let cmds = parse_svg_path_to_draw_commands("M 0 0 Q 2.5 5, 5 0 T 10 0").unwrap();
    assert!(matches!(cmds[2], DrawCommand::SmoothQuadraticBezier { .. }));
    let tracer = build(cmds, false, false, false);

    assert!(matches!(
      tracer.leaves()[0].segments[1],
      PathSegment::Quadratic { .. }
    ));
    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(0.0, 0.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_tessellate_path_from_sequence() {
    let src = r#"
path = [vec2(0, 0), vec2(1, 0), vec2(0, 1)]
mesh = tessellate_path(path)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh = mesh_val.as_mesh().unwrap();
    assert_eq!(mesh.mesh.faces.len(), 1);
  }

  #[test]
  fn test_tessellate_path_from_path_block() {
    let src = r#"
path = path() | move(0, 0) | line(1, 0) | line(1, 1) | line(0, 1) | close
mesh = tessellate_path(path)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh = mesh_val.as_mesh().unwrap();
    assert_eq!(mesh.mesh.vertices.len(), 4);
    assert_eq!(mesh.mesh.faces.len(), 2);
  }

  #[test]
  fn test_tessellate_path_plane_override() {
    // Asymmetric triangle so the u/v axis assignment is observable: 2 units along u, 1 along v.
    fn tessellate(plane: Option<&str>, engine: &str) -> (Vec<Vector3<f32>>, Vector3<f32>) {
      let arg = plane
        .map(|p| format!(", plane=\"{p}\""))
        .unwrap_or_default();
      let src = format!(
        "path = [vec2(0, 0), vec2(2, 0), vec2(0, 1)]\nmesh = tessellate_path(path{arg}, \
         engine=\"{engine}\")\n"
      );
      let ctx = parse_and_eval_program(&src).unwrap();
      let mesh_val = ctx.get_global("mesh").unwrap();
      let lm = &mesh_val.as_mesh().unwrap().mesh;
      let positions = lm.vertices.values().map(|v| v.position).collect();
      let normal = lm.faces.values().next().unwrap().normal(&lm.vertices);
      (positions, normal)
    }

    let approx = |a: f32, b: f32| (a - b).abs() < 1e-5;
    let extent = |ps: &[Vector3<f32>], axis: usize| ps.iter().map(|p| p[axis]).fold(0f32, f32::max);
    let check =
      |plane: Option<&str>, flat_axis: usize, u_axis: usize, v_axis: usize, normal: usize| {
        for engine in ["cgal", "lyon"] {
          let (ps, n) = tessellate(plane, engine);
          assert!(
            ps.iter().all(|p| approx(p[flat_axis], 0.)),
            "{plane:?} {engine} flat"
          );
          assert!(
            approx(extent(&ps, u_axis), 2.),
            "{plane:?} {engine} u extent"
          );
          assert!(
            approx(extent(&ps, v_axis), 1.),
            "{plane:?} {engine} v extent"
          );
          let mut want = Vector3::zeros();
          want[normal] = 1.;
          assert!((n - want).norm() < 1e-5, "{plane:?} {engine} normal {n:?}");
        }
      };

    check(None, 1, 0, 2, 1);
    check(Some("xz"), 1, 0, 2, 1);
    check(Some("xy"), 2, 0, 1, 2);
    check(Some("yz"), 0, 1, 2, 0);
    check(Some("zx"), 1, 2, 0, 1);

    let src = "mesh = tessellate_path([vec2(0,0), vec2(1,0), vec2(0,1)], plane=\"xx\")\n";
    assert!(parse_and_eval_program(src).is_err());
  }

  #[test]
  fn test_subpaths_builtin() {
    let src = r#"
path = path() | move(0, 0) | line(10, 0) | move(100, 0) | line(100, 20)
subs = subpaths(path)
sub_array = collect(subs)
count = len(sub_array)
first = sub_array[0]
first_start = first(0)
first_end = first(1)
second = sub_array[1]
second_start = second(0)
second_end = second(1)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    assert_eq!(ctx.get_global("count").unwrap().as_int().unwrap(), 2);
    assert_vec2_close(global_vec2(&ctx, "first_start"), Vec2::new(0.0, 0.0));
    assert_vec2_close(global_vec2(&ctx, "first_end"), Vec2::new(10.0, 0.0));
    assert_vec2_close(global_vec2(&ctx, "second_start"), Vec2::new(100.0, 0.0));
    assert_vec2_close(global_vec2(&ctx, "second_end"), Vec2::new(100.0, 20.0));
  }

  #[test]
  fn test_lerp_paths_midpoint() {
    let src = r#"
path_a = path() | move(0, 0) | line(2, 0)
path_b = path() | move(0, 2) | line(2, 2)
lerped = lerp_paths(path_a, path_b, 0.5)
result = lerped(0.5)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "result"), Vec2::new(1.0, 1.0));
  }

  #[test]
  fn test_lerp_paths_mix_extremes() {
    let src = r#"
path_a = path() | move(0, 0) | line(4, 0)
path_b = path() | move(0, 10) | line(4, 10)
lerped_a = lerp_paths(path_a, path_b, 0.0)
at_a = lerped_a(0.5)
lerped_b = lerp_paths(path_a, path_b, 1.0)
at_b = lerped_b(0.5)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "at_a"), Vec2::new(2.0, 0.0));
    assert_vec2_close(global_vec2(&ctx, "at_b"), Vec2::new(2.0, 10.0));
  }

  #[test]
  fn test_lerp_paths_critical_point_merging() {
    let src = r#"
path_a = path() | move(0, 0) | line(1, 0) | line(2, 0)
path_b = path() | move(0, 0) | line(0.5, 0) | line(1, 0) | line(2, 0)
lerped = lerp_paths(path_a, path_b, 0.5)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let lerped = ctx.get_global("lerped").unwrap();
    let cps = lerped.as_path().unwrap().critical_t_values();
    assert!(
      cps.len() > 3,
      "expected merged critical points, got {cps:?}"
    );
    assert!((cps[0] - 0.0).abs() < 1e-6);
    assert!((cps[cps.len() - 1] - 1.0).abs() < 1e-6);
    for w in cps.windows(2) {
      assert!(w[0] <= w[1], "Critical points not sorted: {:?}", cps);
    }
  }

  #[test]
  fn test_critical_points_builtin() {
    let src = r#"
path = path() | move(0, 0) | line(1, 0) | line(2, 1)
cps = critical_points(path)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let cps_val = ctx.get_global("cps").unwrap();
    let items: Vec<f32> = cps_val
      .as_sequence()
      .unwrap()
      .consume(&ctx)
      .map(|r| r.unwrap().as_float().unwrap())
      .collect();
    assert!(items.len() >= 3, "got {items:?}");
    assert!((items[0] - 0.0).abs() < 1e-6);
    assert!((items[items.len() - 1] - 1.0).abs() < 1e-6);
    for w in items.windows(2) {
      assert!(w[0] <= w[1], "Critical points not sorted: {:?}", items);
    }
  }

  #[test]
  fn test_critical_points_rejects_non_path() {
    let src = r#"
f = |t| { vec2(t, t) }
cps = critical_points(f)
"#;
    assert!(parse_and_eval_program(src).is_err());
  }

  #[test]
  fn test_path_transform_composition() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 0.0)),
    ];
    let tracer = build(cmds, false, false, false);
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(10.0, 0.0));

    let translated = tracer.transformed(&Matrix3::new(1.0, 0.0, 5.0, 0.0, 1.0, 3.0, 0.0, 0.0, 1.0));
    assert_vec2_close(sample(&translated, 0.0), Vec2::new(5.0, 3.0));
    assert_vec2_close(sample(&translated, 1.0), Vec2::new(15.0, 3.0));

    let cos = std::f32::consts::FRAC_PI_2.cos();
    let sin = std::f32::consts::FRAC_PI_2.sin();
    let rotated =
      translated.transformed(&Matrix3::new(cos, -sin, 0.0, sin, cos, 0.0, 0.0, 0.0, 1.0));
    assert_vec2_close(sample(&rotated, 0.0), Vec2::new(-3.0, 5.0));
  }

  #[test]
  fn test_path_trans_rot_scale_e2e() {
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0 L 10 10 L 0 10 Z")
moved = translate(vec2(5, 3), path)
p0 = moved(0)
p_mid = moved(0.25)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p0"), Vec2::new(5.0, 3.0));
    assert_vec2_close(global_vec2(&ctx, "p_mid"), Vec2::new(15.0, 3.0));

    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
moved = translate(100, 200, path)
p0 = moved(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p0"), Vec2::new(100.0, 200.0));

    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
scaled = scale(2, path)
p0 = scaled(0)
p1 = scaled(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p0"), Vec2::new(0.0, 0.0));
    assert_vec2_close(global_vec2(&ctx, "p1"), Vec2::new(20.0, 0.0));

    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
rotated = rot(pi / 2, path)
p_end = rotated(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p_end"), Vec2::new(0.0, 10.0));

    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
result = rot(pi / 2, translate(vec2(5, 0), path))
p0 = result(0)
p1 = result(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p0"), Vec2::new(0.0, 5.0));
    assert_vec2_close(global_vec2(&ctx, "p1"), Vec2::new(0.0, 15.0));
  }

  #[test]
  fn test_path_reflect_e2e() {
    let src = r#"
path = trace_svg_path("M 2 3 L 8 3")
rx = reflect_x(path)
rx_off = reflect_x(10, path)
ry = reflect_y(path)
ry_off = path | reflect_y(10)
diag = reflect(vec2(1, 1), path)
diag_off = reflect(vec2(1, 0), 4, path)
rx0 = rx(0)
rx1 = rx(1)
rx_off0 = rx_off(0)
ry0 = ry(0)
ry_off0 = ry_off(0)
ry_off1 = ry_off(1)
diag0 = diag(0)
diag1 = diag(1)
diag_off0 = diag_off(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let g = |name: &str| global_vec2(&ctx, name);

    assert_vec2_close(g("rx0"), Vec2::new(2.0, -3.0));
    assert_vec2_close(g("rx1"), Vec2::new(8.0, -3.0));
    assert_vec2_close(g("rx_off0"), Vec2::new(2.0, 17.0));
    assert_vec2_close(g("ry0"), Vec2::new(-2.0, 3.0));
    assert_vec2_close(g("ry_off0"), Vec2::new(18.0, 3.0));
    assert_vec2_close(g("ry_off1"), Vec2::new(12.0, 3.0));
    assert_vec2_close(g("diag0"), Vec2::new(3.0, 2.0));
    assert_vec2_close(g("diag1"), Vec2::new(3.0, 8.0));
    assert_vec2_close(g("diag_off0"), Vec2::new(2.0, 5.0));
  }

  #[test]
  fn test_apply_transforms_and_origin_to_geometry_for_paths() {
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0 L 10 10 L 0 10 Z")
moved = translate(vec2(100, 200), path)
baked = apply_transforms(moved)
p0 = baked(0)
p1 = baked(0.25)
moved_again = translate(vec2(1, 1), baked)
q0 = moved_again(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p0"), Vec2::new(100.0, 200.0));
    assert_vec2_close(global_vec2(&ctx, "p1"), Vec2::new(110.0, 200.0));
    assert_vec2_close(global_vec2(&ctx, "q0"), Vec2::new(101.0, 201.0));

    let src = r#"
path = trace_svg_path("M 10 10 L 20 10 L 20 20 L 10 20 Z")
centered = origin_to_geometry(path)
p0 = centered(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p0"), Vec2::new(-5.0, -5.0));
  }

  /// Explicit (producer) anchors become mandatory boundaries for budget-limited resampling,
  /// while authored joints leave that to the adaptive sampler.
  #[test]
  fn test_resample_boundaries_from_anchors() {
    let square = vec![
      Vec2::new(0.0, 0.0),
      Vec2::new(1.0, 0.0),
      Vec2::new(1.0, 1.0),
      Vec2::new(0.0, 1.0),
    ];
    let marked = Path::from_polylines(
      vec![
        (square.clone(), true),
        (vec![Vec2::new(10.0, 0.0), Vec2::new(11.0, 0.0)], false),
      ],
      Some(vec![vec![true, false, true, false], vec![true, true]]),
    );
    let leaves = marked.leaves();
    let local_0 = leaves[0].resample_boundaries();
    assert_eq!(local_0, vec![0.0, 0.5, 1.0]);
    assert_eq!(leaves[1].resample_boundaries(), vec![0.0, 1.0]);

    let authored = Path::from_polylines(vec![(square, true)], None);
    assert_eq!(authored.leaves()[0].resample_boundaries(), vec![0.0, 1.0]);
  }

  /// `sample_subpaths_with_limit` on an all-line path returns exactly `limit` points when
  /// reducing and the natural samples otherwise.
  #[test]
  fn test_sample_subpaths_with_limit_respects_count_and_corners() {
    use std::f32::consts::TAU;
    let n = 60usize;
    let points: Vec<Vec2> = (0..n)
      .map(|i| {
        let angle = TAU * i as f32 / n as f32;
        Vec2::new(angle.cos(), angle.sin())
      })
      .collect();
    let anchors: Vec<bool> = (0..n).map(|i| i % 15 == 0).collect();
    let tracer = Path::from_polylines(vec![(points, true)], Some(vec![anchors]));
    let ctx = EvalCtx::default();

    let natural = tracer.sample_subpaths(0.1, 64, &ctx).unwrap();
    let natural_total: usize = natural.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(natural_total, 60);

    let limited = tracer
      .sample_subpaths_with_limit(0.1, Some(20), &ctx)
      .unwrap();
    let limited_total: usize = limited.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(
      limited_total, 20,
      "expected exactly 20 points, got {limited_total}"
    );

    let no_reduction = tracer
      .sample_subpaths_with_limit(0.1, Some(100), &ctx)
      .unwrap();
    let no_reduction_total: usize = no_reduction.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(no_reduction_total, natural_total);

    let unlimited = tracer.sample_subpaths_with_limit(0.1, None, &ctx).unwrap();
    let unlimited_total: usize = unlimited.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(unlimited_total, natural_total);
  }

  #[test]
  fn test_rect_subpath_corners() {
    // 4x2 rect centered at origin, perimeter 12, traced CCW from the top-right corner.
    let tracer = Path::leaf(rect_subpath(Vec2::new(0.0, 0.0), 4.0, 2.0, false));

    assert_vec2_close(sample(&tracer, 0.0), Vec2::new(2.0, 1.0));
    assert_vec2_close(sample(&tracer, 1.0 / 3.0), Vec2::new(-2.0, 1.0));
    assert_vec2_close(sample(&tracer, 0.5), Vec2::new(-2.0, -1.0));
    assert_vec2_close(sample(&tracer, 5.0 / 6.0), Vec2::new(2.0, -1.0));
    assert_vec2_close(sample(&tracer, 1.0), Vec2::new(2.0, 1.0));
  }

  #[test]
  fn test_rect_via_path_block_scalar_size() {
    let src = r#"
path = rect(center=v2(0, 0), size=2)
mesh = tessellate_path(path)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let mesh = ctx.get_global("mesh").unwrap();
    let mesh = mesh.as_mesh().unwrap();
    assert_eq!(mesh.mesh.vertices.len(), 4);
    assert_eq!(mesh.mesh.faces.len(), 2);
  }

  #[test]
  fn test_rect_via_path_block_vec2_size() {
    let src = r#"
path = rect(center=v2(1, 2), size=v2(4, 6))
tr = path(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "tr"), Vec2::new(3.0, 5.0));
  }

  #[test]
  fn test_rect_via_path_block_numeric_form() {
    let src = r#"
path = rect(0, 0, 4, 2)
tr = path(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "tr"), Vec2::new(2.0, 1.0));
  }

  #[test]
  fn test_path_join_concatenates_subpaths() {
    let src = r#"
a = path() | move(0, 0) | line(1, 0)
b = path() | move(10, 0) | line(11, 0)
joined = path([a, b])
p_a = joined(0.25)
p_b = joined(0.75)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p_a"), Vec2::new(0.5, 0.0));
    assert_vec2_close(global_vec2(&ctx, "p_b"), Vec2::new(10.5, 0.0));
  }

  #[test]
  fn test_path_join_bakes_non_identity_transforms() {
    let src = r#"
a = path() | move(0, 0) | line(1, 0) | translate(v2(5, 0))
b = path() | move(0, 0) | line(1, 0) | translate(v2(20, 0))
joined = path([a, b])
p_a = joined(0.25)
p_b = joined(0.75)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "p_a"), Vec2::new(5.5, 0.0));
    assert_vec2_close(global_vec2(&ctx, "p_b"), Vec2::new(20.5, 0.0));
  }

  #[test]
  fn test_build_segment_dicts_lines_and_global_t() {
    // Two open subpaths with lengths 1 and 3 → global total = 4.
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(0.5, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::MoveTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(13.0, 0.0)),
    ];
    let tracer = build(cmds, false, false, false);
    let dicts = build_segment_dicts(&tracer, "test").unwrap();
    let unwrap_map = |v: &Value| match v {
      Value::Map(m) => m.clone(),
      _ => panic!("expected Map, got {v:?}"),
    };
    let f = |m: &ValueMap, k: &str| m.get(k).and_then(|v| v.as_float()).unwrap();
    let i = |m: &ValueMap, k: &str| m.get(k).and_then(|v| v.as_int()).unwrap();
    let s = |m: &ValueMap, k: &str| {
      m.get(k)
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap()
    };

    assert_eq!(dicts.len(), 3);

    let m0 = unwrap_map(&dicts[0]);
    assert_eq!(s(&m0, "type"), "line");
    assert_eq!(i(&m0, "subpath"), 0);
    assert!((f(&m0, "t_start") - 0.0).abs() < 1e-6);
    assert!((f(&m0, "t_end") - 0.5).abs() < 1e-6);
    assert!((f(&m0, "t_end_global") - 0.125).abs() < 1e-6);

    let m2 = unwrap_map(&dicts[2]);
    assert_eq!(i(&m2, "subpath"), 1);
    assert!((f(&m2, "t_start") - 0.0).abs() < 1e-6);
    assert!((f(&m2, "t_end") - 1.0).abs() < 1e-6);
    assert!((f(&m2, "t_start_global") - 0.25).abs() < 1e-6);
    assert!((f(&m2, "t_end_global") - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_path_segments_end_to_end() {
    let src = r#"
p = path() | move(0, 0) | line(1, 0) | line(1, 1)
segs = path_segments(p)
types = segs -> |s, _i| s.type
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let types = ctx.get_global("types").unwrap();
    let collected: Vec<String> = types
      .as_sequence()
      .unwrap()
      .consume(&ctx)
      .map(|r| r.unwrap().as_str().unwrap().to_owned())
      .collect();
    assert_eq!(collected, vec!["line".to_owned(), "line".to_owned()]);
  }

  #[test]
  fn test_path_len_analytic_fold_and_lazy() {
    let src = r#"
poly = path() | move(0, 0) | line(3, 0) | line(3, 4)
poly_len = len(poly)

scaled = poly | scale(2)
scaled_len = len(scaled)

circle = circle(v2(0), 3)
fast = len(circle)
slow = path_segments(circle) | fold(0, |acc, s| acc + s.length)

lazy = len(lerp_paths(poly, poly, 0.5))
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let g = |name: &str| ctx.get_global(name).unwrap().as_float().unwrap();

    assert!(
      (g("poly_len") - 7.0).abs() < 1e-4,
      "poly_len = {}",
      g("poly_len")
    );
    assert!(
      (g("scaled_len") - 14.0).abs() < 1e-4,
      "scaled_len = {}",
      g("scaled_len")
    );
    assert!(
      (g("fast") - g("slow")).abs() < 1e-3,
      "fast={} slow={}",
      g("fast"),
      g("slow")
    );
    assert!(
      (g("fast") - 2.0 * PI * 3.0).abs() < 0.2,
      "circle len {} not ~= circumference",
      g("fast")
    );
    assert!((g("lazy") - 7.0).abs() < 1e-2, "lazy = {}", g("lazy"));
  }

  #[test]
  fn test_trim_path_preserves_corner_and_endpoints() {
    // Two perpendicular segments meeting at (4,0), total length 8. Trimming [2, 6] by distance
    // keeps (2,0)->(4,0)->(4,2) with the corner intact.
    let src = r#"
p = path() | move(0, 0) | line(4, 0) | line(4, 4)
t = trim_path(p, start=2, end=6, unit='distance')
a = t(0)
mid = t(0.5)
b = t(1)
tlen = len(t)
types = path_segments(t) -> |s, _i| s.type
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert_vec2_close(global_vec2(&ctx, "a"), Vec2::new(2.0, 0.0));
    assert_vec2_close(global_vec2(&ctx, "mid"), Vec2::new(4.0, 0.0));
    assert_vec2_close(global_vec2(&ctx, "b"), Vec2::new(4.0, 2.0));

    let tlen = ctx.get_global("tlen").unwrap().as_float().unwrap();
    assert!((tlen - 4.0).abs() < 1e-4, "trimmed length = {tlen}");

    let types: Vec<String> = ctx
      .get_global("types")
      .unwrap()
      .as_sequence()
      .unwrap()
      .consume(&ctx)
      .map(|r| r.unwrap().as_str().unwrap().to_owned())
      .collect();
    assert_eq!(types, vec!["line".to_owned(), "line".to_owned()]);
  }

  #[test]
  fn test_path_frame_unit_circle_inward_normal() {
    let src = r#"
p = circle(v2(0), 1)
f = path_frame(0.25, p)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let frame = ctx.get_global("f").unwrap();
    let map = match frame {
      Value::Map(m) => m,
      _ => panic!("expected Map"),
    };

    let pos = *map.get("pos").and_then(|v| v.as_vec2()).unwrap();
    let tangent = *map.get("tangent").and_then(|v| v.as_vec2()).unwrap();
    let normal = *map.get("normal").and_then(|v| v.as_vec2()).unwrap();

    assert!(
      (pos.norm() - 1.0).abs() < 1e-2,
      "pos not on unit circle: {pos:?}"
    );
    assert!(
      tangent.dot(&pos).abs() < 5e-2,
      "tangent not perp to radial: tangent={tangent:?}, pos={pos:?}"
    );
    let inward = -pos.normalize();
    assert!(
      (normal - inward).norm() < 5e-2,
      "normal not inward: normal={normal:?}, expected~={inward:?}"
    );
  }

  #[test]
  fn test_path_frame_rejects_non_path() {
    let src = r#"
p = |t| v2(cos(t * tau), sin(t * tau))
f = path_frame(0.25, p)
"#;
    assert!(parse_and_eval_program(src).is_err());
  }

  #[test]
  fn test_path_frame_on_lazy_path() {
    let src = r#"
p = catmull_rom([v2(1, 0), v2(0, 1), v2(-1, 0), v2(0, -1)], closed=true)
f = path_frame(0.25, p)
g = path_frame(0.25, p, inward_normal=false)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    assert!(matches!(ctx.get_global("f").unwrap(), Value::Map(_)));
    assert!(matches!(ctx.get_global("g").unwrap(), Value::Map(_)));
  }

  #[test]
  fn test_discretize_path_replaces_curves_with_lines() {
    let src = r#"
p = circle(v2(0), 5)
disc = discretize_path(p, curve_angle_degrees=2)
segs = path_segments(disc)
types = segs -> |s, _i| s.type
closed_flags = segs -> |s, _i| s.closed
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let types_seq = ctx.get_global("types").unwrap();
    let collected: Vec<String> = types_seq
      .as_sequence()
      .unwrap()
      .consume(&ctx)
      .map(|r| r.unwrap().as_str().unwrap().to_owned())
      .collect();
    assert!(
      collected.len() > 8,
      "expected many line segments, got {collected:?}"
    );
    assert!(
      collected.iter().all(|t| t == "line"),
      "non-line segment in {collected:?}"
    );

    let closed_seq = ctx.get_global("closed_flags").unwrap();
    let closed_collected: Vec<bool> = closed_seq
      .as_sequence()
      .unwrap()
      .consume(&ctx)
      .map(|r| r.unwrap().as_bool().unwrap())
      .collect();
    assert!(
      closed_collected.iter().all(|c| *c),
      "expected discretized circle to remain closed"
    );

    let disc = ctx.get_global("disc").unwrap();
    let cps = disc.as_path().unwrap().critical_t_values();
    assert_eq!(
      cps.len(),
      3,
      "arc joints stay anchored, interior vertices don't: {cps:?}"
    );
  }

  #[test]
  fn test_build_segment_dicts_arc_fields() {
    let tracer = Path::leaf(circle_subpath(Vec2::new(0.0, 0.0), 5.0, false));
    let dicts = build_segment_dicts(&tracer, "test").unwrap();
    assert_eq!(dicts.len(), 2);

    for d in &dicts {
      let Value::Map(m) = d else {
        panic!("expected Map, got {d:?}");
      };
      assert_eq!(m.get("type").and_then(|v| v.as_str()).unwrap(), "arc");
      assert!((m.get("rx").and_then(|v| v.as_float()).unwrap() - 5.0).abs() < 1e-4);
      assert!(!m.get("large_arc").and_then(|v| v.as_bool()).unwrap());
      let theta_delta = m.get("theta_delta").and_then(|v| v.as_float()).unwrap();
      assert!((theta_delta.abs() - PI).abs() < 1e-3);
      let rotation = m.get("x_axis_rotation").and_then(|v| v.as_float()).unwrap();
      assert!(rotation.abs() < 1e-3);
    }
  }
}
