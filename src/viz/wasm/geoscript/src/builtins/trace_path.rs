use std::any::Any;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::f32::consts::PI;
use std::rc::Rc;

use fxhash::FxHashMap;
use svgtypes::PathParser;

use nalgebra::{Matrix3, Vector3};

use crate::{
  ArgRef, ArgType, Callable, DynamicCallable, ErrorStack, EvalCtx, Sequence, Sym, Value, Vec2,
  EMPTY_KWARGS,
};

const CURVE_TABLE_SAMPLES: usize = 32;
const LENGTH_EPSILON: f32 = 1e-5;

#[derive(Clone, Debug)]
pub(crate) struct ArcLengthTable {
  cumulative: Vec<f32>,
  total: f32,
}

impl ArcLengthTable {
  fn new(samples: usize, mut sample_fn: impl FnMut(f32) -> Vec2) -> Self {
    let samples = samples.max(1);
    let mut cumulative = Vec::with_capacity(samples + 1);
    let mut total = 0.0;

    let mut prev = sample_fn(0.0);
    cumulative.push(0.0);

    for i in 1..=samples {
      let t = i as f32 / samples as f32;
      let point = sample_fn(t);
      total += (point - prev).norm();
      cumulative.push(total);
      prev = point;
    }

    Self { cumulative, total }
  }

  fn total(&self) -> f32 {
    self.total
  }

  fn param_for_length(&self, length: f32) -> f32 {
    if self.total <= LENGTH_EPSILON {
      return 0.0;
    }
    let target = length.clamp(0.0, self.total);
    let idx = match self
      .cumulative
      .binary_search_by(|val| val.partial_cmp(&target).unwrap_or(Ordering::Less))
    {
      Ok(ix) => ix,
      Err(ix) => ix,
    };
    if idx == 0 {
      return 0.0;
    }
    if idx >= self.cumulative.len() {
      return 1.0;
    }

    let prev = self.cumulative[idx - 1];
    let next = self.cumulative[idx];
    let span = next - prev;
    let alpha = if span <= 0.0 {
      0.0
    } else {
      (target - prev) / span
    };
    let samples = (self.cumulative.len() - 1) as f32;
    let t0 = (idx - 1) as f32 / samples;
    let t1 = idx as f32 / samples;
    t0 + (t1 - t0) * alpha
  }
}

#[derive(Clone, Debug)]
pub(crate) enum PathSegment {
  Line {
    start: Vec2,
    end: Vec2,
    length: f32,
  },
  Quadratic {
    start: Vec2,
    ctrl: Vec2,
    end: Vec2,
    table: ArcLengthTable,
  },
  Cubic {
    start: Vec2,
    ctrl1: Vec2,
    ctrl2: Vec2,
    end: Vec2,
    table: ArcLengthTable,
  },
  Arc {
    end: Vec2,
    center: Vec2,
    rx: f32,
    ry: f32,
    cos_phi: f32,
    sin_phi: f32,
    theta_start: f32,
    theta_delta: f32,
    table: ArcLengthTable,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FillRule {
  NonZero,
  EvenOdd,
  Positive,
  Negative,
}

impl FillRule {
  pub(crate) fn parse(value: &Value, fn_name: &str) -> Result<Self, ErrorStack> {
    if let Some(s) = value.as_str() {
      let key = s.to_ascii_lowercase();
      return match key.as_str() {
        "nonzero" | "non_zero" | "non-zero" => Ok(FillRule::NonZero),
        "evenodd" | "even_odd" | "even-odd" => Ok(FillRule::EvenOdd),
        "positive" => Ok(FillRule::Positive),
        "negative" => Ok(FillRule::Negative),
        _ => Err(ErrorStack::new(format!(
          "Invalid fill_rule for `{fn_name}`; expected one of \"nonzero\", \"evenodd\", \
           \"positive\", \"negative\", found: \"{s}\""
        ))),
      };
    }

    if let Some(num) = value.as_float() {
      let num = num as f64;
      if !(0.0..=3.0).contains(&num) {
        return Err(ErrorStack::new(format!(
          "Invalid fill_rule for `{fn_name}`; expected in [0, 3], found: {num}"
        )));
      }
      return FillRule::from_clipper2_u32(num as u32).ok_or_else(|| {
        ErrorStack::new(format!(
          "Invalid fill_rule for `{fn_name}`; unexpected numeric value: {num}"
        ))
      });
    }

    Err(ErrorStack::new(format!(
      "Invalid fill_rule for `{fn_name}`; expected string or number, found: {value:?}"
    )))
  }

  #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
  pub(crate) fn to_clipper2_u32(self) -> u32 {
    match self {
      FillRule::EvenOdd => 0,
      FillRule::NonZero => 1,
      FillRule::Positive => 2,
      FillRule::Negative => 3,
    }
  }

  #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
  pub(crate) fn from_clipper2_u32(val: u32) -> Option<Self> {
    match val {
      0 => Some(FillRule::EvenOdd),
      1 => Some(FillRule::NonZero),
      2 => Some(FillRule::Positive),
      3 => Some(FillRule::Negative),
      _ => None,
    }
  }

  pub(crate) fn to_lyon_fill_rule(self) -> Result<lyon_tessellation::FillRule, ErrorStack> {
    match self {
      FillRule::NonZero => Ok(lyon_tessellation::FillRule::NonZero),
      FillRule::EvenOdd => Ok(lyon_tessellation::FillRule::EvenOdd),
      FillRule::Positive | FillRule::Negative => Err(ErrorStack::new(
        "fill_rule \"positive\" and \"negative\" are Clipper2-only and are not supported for \
         polygon tessellation; use \"nonzero\" or \"evenodd\"",
      )),
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SegmentInterval {
  pub end: f32,
  pub has_detail: bool,
}

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
pub(crate) fn normalize_path_sampler_guides(sampler: &dyn PathSampler) -> (Vec<f32>, f32) {
  let raw_cps = sampler.critical_t_values();
  let is_closed_single = sampler
    .subpath_topology()
    .map(|t| t.len() == 1 && t[0].closed)
    .unwrap_or(false);

  if !is_closed_single {
    return (normalize_guides(&raw_cps), 0.0);
  }

  // Find the earliest critical point. If it's already at (or very near) 0, no rotation needed.
  let t_min = raw_cps
    .iter()
    .copied()
    .filter(|v| v.is_finite() && *v >= 0.0 && *v <= 1.0)
    .fold(f32::INFINITY, f32::min);

  if t_min.is_infinite() || t_min <= GUIDE_EPSILON {
    return (normalize_guides(&raw_cps), 0.0);
  }

  // Rotate: t' = (t - t_min) mod 1.0. After rotation, t_min maps to 0.0.
  let rotated: Vec<f32> = raw_cps
    .iter()
    .map(|&t| (t - t_min).rem_euclid(1.0))
    .collect();
  (normalize_guides(&rotated), t_min)
}

pub(crate) struct SubpathTopology {
  pub closed: bool,
  pub segment_count: usize,
}

pub(crate) trait PathSampler: Any {
  fn critical_t_values(&self) -> Vec<f32>;
  fn subpath_topology(&self) -> Option<Vec<SubpathTopology>> {
    None
  }
  fn fill_rule(&self) -> Option<FillRule> {
    None
  }

  /// Returns the 2D affine transform matrix for this path sampler.
  fn transform(&self) -> &Matrix3<f32>;

  /// Returns a new path sampler (as a DynamicCallable) with the given transform composed
  /// (left-multiplied) onto the existing transform.
  fn with_transform(&self, t: Matrix3<f32>) -> Box<dyn DynamicCallable>;

  /// Evaluates the path at parameter `t` in local (untransformed) space.
  fn eval_at_raw(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack>;

  /// Evaluates the path at parameter `t` with the transform applied.
  fn eval_at(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    let p = self.eval_at_raw(t, ctx)?;
    Ok(apply_transform_to_point(self.transform(), p))
  }

  /// Fast-path: sample all subpath points with topology-aware adaptive subdivision.
  /// Returns `None` if not supported (e.g. black-box callables).
  /// Each entry is (points, is_closed).  Points are in world (transformed) space.
  fn sample_subpaths(&self, _angle_tolerance: f32) -> Option<Vec<(Vec<Vec2>, bool)>> {
    None
  }

  /// Like `sample_subpaths`, but caps the total number of output points across all subpaths.
  ///
  /// When `total_limit` is `Some(n)` and the natural tessellation would exceed `n` points,
  /// adaptively resamples each subpath using curvature+arc-length weighting, preserving any
  /// detected topological critical points (sharp corners) as mandatory boundaries.
  ///
  /// When `total_limit` is `None` or the natural count is already within the limit, delegates
  /// to `sample_subpaths` unchanged.
  fn sample_subpaths_with_limit(
    &self,
    angle_tolerance: f32,
    _total_limit: Option<usize>,
  ) -> Option<Vec<(Vec<Vec2>, bool)>> {
    self.sample_subpaths(angle_tolerance)
  }

  /// Build a lyon `Path` directly from the sampler's curve topology (lines, beziers, arcs),
  /// with the sampler's transform applied to all control points.
  ///
  /// Returns `None` for black-box callables that don't expose curve topology.
  /// When `Some` is returned, the caller can pass the lyon `Path` directly to the fill
  /// tessellator, giving lyon visibility into the actual curve geometry for more precise
  /// intersection detection and fill-rule handling.
  fn to_lyon_path_for_tessellation(&self) -> Option<lyon_tessellation::path::Path> {
    None
  }
}

/// Applies a 2D affine transform (3x3 homogeneous matrix) to a point.
/// Returns the point unchanged if the matrix is identity.
pub(crate) fn apply_transform_to_point(m: &Matrix3<f32>, p: Vec2) -> Vec2 {
  if *m == Matrix3::identity() {
    return p;
  }
  let tp = m * Vector3::new(p.x, p.y, 1.0);
  Vec2::new(tp.x, tp.y)
}

fn parse_path_sampler_t_arg<'a>(
  args: &'a [Value],
  kwargs: &'a FxHashMap<Sym, Value>,
  interned_t_kwarg: Sym,
) -> Result<f32, ErrorStack> {
  let t = if !kwargs.is_empty() {
    if kwargs.len() != 1 || !kwargs.contains_key(&interned_t_kwarg) {
      return Err(ErrorStack::new(
        "Unexpected keyword arguments; expected only `t`",
      ));
    }
    if !args.is_empty() {
      return Err(ErrorStack::new(
        "Expected only keyword argument `t` and no positional args",
      ));
    }
    kwargs.get(&interned_t_kwarg).unwrap()
  } else {
    if args.len() < 1 {
      return Err(ErrorStack::new("Expected argument `t`"));
    }
    &args[0]
  };
  let Some(t) = t.as_float() else {
    return Err(ErrorStack::new(format!(
      "Expected 't' to be a number, found {t:?}"
    )));
  };
  Ok(t.clamp(0., 1.))
}

impl<T: PathSampler + 'static> DynamicCallable for T {
  fn as_any(&self) -> &dyn Any {
    self
  }

  fn invoke(
    &self,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let interned_t = ctx.interned_symbols.intern("t");
    let t = parse_path_sampler_t_arg(args, kwargs, interned_t)?;
    let pos = self.eval_at(t, ctx)?;
    Ok(Value::Vec2(pos))
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

pub(crate) fn as_path_tracer(callable: &Callable) -> Option<&PathTracerCallable> {
  match callable {
    Callable::Dynamic { inner, .. } => inner.as_any().downcast_ref::<PathTracerCallable>(),
    _ => None,
  }
}

pub(crate) fn as_path_sampler(callable: &Callable) -> Option<&dyn PathSampler> {
  match callable {
    Callable::Dynamic { inner, .. } => {
      let any = inner.as_any();
      any
        .downcast_ref::<PathTracerCallable>()
        .map(|t| t as &dyn PathSampler)
        .or_else(|| {
          any
            .downcast_ref::<super::lerp_path::LerpPathCallable>()
            .map(|t| t as &dyn PathSampler)
        })
        .or_else(|| {
          any
            .downcast_ref::<TransformedCallableSampler>()
            .map(|t| t as &dyn PathSampler)
        })
        .or_else(|| {
          any
            .downcast_ref::<super::catmull_rom::CatmullRomCallable2D>()
            .map(|t| t as &dyn PathSampler)
        })
    }
    _ => None,
  }
}

/// Wraps any `Callable` (including arbitrary `|t| -> Vec2` functions) with a 2D affine transform.
/// This allows transforms to be applied to black-box path functions that don't implement
/// `PathSampler` natively.
pub(crate) struct TransformedCallableSampler {
  pub inner: Rc<Callable>,
  pub transform: Matrix3<f32>,
  pub cached_critical_points: Vec<f32>,
}

impl PathSampler for TransformedCallableSampler {
  fn critical_t_values(&self) -> Vec<f32> {
    self.cached_critical_points.clone()
  }

  fn transform(&self) -> &Matrix3<f32> {
    &self.transform
  }

  fn with_transform(&self, t: Matrix3<f32>) -> Box<dyn DynamicCallable> {
    Box::new(TransformedCallableSampler {
      inner: Rc::clone(&self.inner),
      transform: t * self.transform,
      cached_critical_points: self.cached_critical_points.clone(),
    })
  }

  fn eval_at_raw(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    let val = ctx
      .invoke_callable(&self.inner, &[Value::Float(t)], EMPTY_KWARGS)
      .map_err(|e| e.wrap("Error invoking callable in TransformedCallableSampler"))?;
    val
      .as_vec2()
      .copied()
      .ok_or_else(|| ErrorStack::new("TransformedCallableSampler: callable did not return a Vec2"))
  }
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

pub(crate) fn build_interval_weights(
  guides: &[f32],
  sampler_intervals: &[Vec<SegmentInterval>],
) -> Option<Vec<f32>> {
  if guides.len() < 2 || sampler_intervals.is_empty() {
    return None;
  }

  let mut indices = vec![0usize; sampler_intervals.len()];
  let mut weights = Vec::with_capacity(guides.len() - 1);
  for &[start, end] in guides.array_windows::<2>() {
    let mid = (start + end) * 0.5;
    let mut all_detail = true;

    for (sampler_ix, intervals) in sampler_intervals.iter().enumerate() {
      if intervals.is_empty() {
        all_detail = false;
        continue;
      }

      let mut idx = indices[sampler_ix];
      while idx + 1 < intervals.len() && mid > intervals[idx].end + GUIDE_EPSILON {
        idx += 1;
      }
      indices[sampler_ix] = idx;

      if !intervals[idx].has_detail {
        all_detail = false;
      }
    }

    weights.push(if all_detail { 1.0 } else { 0.0 });
  }

  Some(weights)
}

/// Discretizes a path callable into per-subpath polylines.
///
/// Returns one `(points, is_closed)` entry per subpath. For paths backed by a `PathSampler`
/// implementation that exposes subpath topology, uses adaptive curvature-based sampling driven
/// by `curve_angle_radians`. For black-box `|t: num|: vec2` callables, falls back to a single
/// subpath of `sample_count` uniform samples; closedness is inferred from `p(0) ≈ p(1)`
/// unless `closed_override` is provided.
///
/// Subpaths whose discretization produces fewer than 2 points are filtered out.
pub(crate) fn sample_path_subpaths(
  ctx: &EvalCtx,
  path_callable: &Rc<Callable>,
  curve_angle_radians: f32,
  sample_count: usize,
  closed_override: Option<bool>,
  fn_name: &str,
) -> Result<Vec<(Vec<Vec2>, bool)>, ErrorStack> {
  let sampler = as_path_sampler(path_callable);
  let subpath_data = sampler.and_then(|s| s.sample_subpaths(curve_angle_radians));

  if let Some(subpath_data) = subpath_data {
    let mut out = Vec::with_capacity(subpath_data.len());
    for (points, is_closed) in subpath_data {
      if points.len() >= 2 {
        out.push((points, closed_override.unwrap_or(is_closed)));
      }
    }
    return Ok(out);
  }

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
    Ok(vec![(points, is_closed)])
  } else {
    Ok(Vec::new())
  }
}

/// Converts per-subpath polylines into a flat sequence of `MoveTo` / `LineTo` / `Close`
/// `DrawCommand`s, suitable for feeding into `PathTracerCallable::new`.
///
/// Subpaths with fewer than 2 points are skipped. For closed subpaths whose first and last
/// points coincide (within 1e-6), the duplicate trailing vertex is dropped before emitting
/// the `Close` command.
pub(crate) fn polylines_to_draw_commands(
  subpaths: impl Iterator<Item = (Vec<Vec2>, bool)>,
) -> Vec<DrawCommand> {
  let mut cmds = Vec::new();
  for (mut points, is_closed) in subpaths {
    if points.len() < 2 {
      continue;
    }
    if is_closed {
      if let (Some(first), Some(last)) = (points.first(), points.last()) {
        if (*first - *last).norm() <= 1e-6 {
          points.pop();
        }
      }
    }
    let Some(first) = points.first().copied() else {
      continue;
    };
    cmds.push(DrawCommand::MoveTo(first));
    for pt in points.iter().skip(1) {
      cmds.push(DrawCommand::LineTo(*pt));
    }
    if is_closed {
      cmds.push(DrawCommand::Close);
    }
  }
  cmds
}

impl PathSegment {
  pub(crate) fn translate(&mut self, offset: Vec2) {
    match self {
      PathSegment::Line { start, end, .. } => {
        *start = *start + offset;
        *end = *end + offset;
      }
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => {
        *start = *start + offset;
        *ctrl = *ctrl + offset;
        *end = *end + offset;
      }
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => {
        *start = *start + offset;
        *ctrl1 = *ctrl1 + offset;
        *ctrl2 = *ctrl2 + offset;
        *end = *end + offset;
      }
      PathSegment::Arc { center, end, .. } => {
        *center = *center + offset;
        *end = *end + offset;
      }
    }
  }

  fn length(&self) -> f32 {
    match self {
      PathSegment::Line { length, .. } => *length,
      PathSegment::Quadratic { table, .. } => table.total(),
      PathSegment::Cubic { table, .. } => table.total(),
      PathSegment::Arc { table, .. } => table.total(),
    }
  }

  fn has_detail(&self) -> bool {
    !matches!(self, PathSegment::Line { .. })
  }

  pub(crate) fn end(&self) -> Vec2 {
    match self {
      PathSegment::Line { end, .. } => *end,
      PathSegment::Quadratic { end, .. } => *end,
      PathSegment::Cubic { end, .. } => *end,
      PathSegment::Arc { end, .. } => *end,
    }
  }

  pub(crate) fn start_point(&self) -> Vec2 {
    match self {
      PathSegment::Line { start, .. } => *start,
      PathSegment::Quadratic { start, .. } => *start,
      PathSegment::Cubic { start, .. } => *start,
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        ..
      } => {
        let (sin_theta, cos_theta) = theta_start.sin_cos();
        let x = rx * cos_theta;
        let y = ry * sin_theta;
        Vec2::new(
          cos_phi * x - sin_phi * y + center.x,
          sin_phi * x + cos_phi * y + center.y,
        )
      }
    }
  }

  /// Returns the exact AABB of this segment in its local coordinate space.
  pub(crate) fn aabb(&self) -> (Vec2, Vec2) {
    match self {
      PathSegment::Line { start, end, .. } => (
        Vec2::new(start.x.min(end.x), start.y.min(end.y)),
        Vec2::new(start.x.max(end.x), start.y.max(end.y)),
      ),
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => quadratic_bezier_aabb(*start, *ctrl, *end),
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => cubic_bezier_aabb(*start, *ctrl1, *ctrl2, *end),
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        theta_delta,
        ..
      } => arc_aabb(
        *center,
        *rx,
        *ry,
        *cos_phi,
        *sin_phi,
        *theta_start,
        *theta_delta,
      ),
    }
  }

  /// Returns the exact AABB of this segment after applying the given 2D affine transform.
  /// Errors only for arc segments under a non-uniform transform, where the result is not an
  /// arc and an exact bound would require evaluating a transformed conic.
  pub(crate) fn aabb_under_transform(&self, m: &Matrix3<f32>) -> Result<(Vec2, Vec2), ErrorStack> {
    if *m == Matrix3::identity() {
      return Ok(self.aabb());
    }
    match self {
      PathSegment::Line { start, end, .. } => {
        let s = apply_transform_to_point(m, *start);
        let e = apply_transform_to_point(m, *end);
        Ok((
          Vec2::new(s.x.min(e.x), s.y.min(e.y)),
          Vec2::new(s.x.max(e.x), s.y.max(e.y)),
        ))
      }
      PathSegment::Quadratic {
        start, ctrl, end, ..
      } => Ok(quadratic_bezier_aabb(
        apply_transform_to_point(m, *start),
        apply_transform_to_point(m, *ctrl),
        apply_transform_to_point(m, *end),
      )),
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        ..
      } => Ok(cubic_bezier_aabb(
        apply_transform_to_point(m, *start),
        apply_transform_to_point(m, *ctrl1),
        apply_transform_to_point(m, *ctrl2),
        apply_transform_to_point(m, *end),
      )),
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        theta_delta,
        ..
      } => {
        if !is_uniform_transform(m) {
          return Err(ErrorStack::new(
            "exact AABB of an arc segment under a non-uniform transform (e.g. non-uniform scale \
             or skew) is not supported; bake the transform into the path first or convert arcs to \
             cubic beziers",
          ));
        }
        let cos_a = m[(0, 0)];
        let sin_a = m[(1, 0)];
        let scale = (cos_a * cos_a + sin_a * sin_a).sqrt();
        let rot_angle = sin_a.atan2(cos_a);
        let new_center = apply_transform_to_point(m, *center);
        let new_rx = rx * scale;
        let new_ry = ry * scale;
        let old_phi = sin_phi.atan2(*cos_phi);
        let new_phi = old_phi + rot_angle;
        Ok(arc_aabb(
          new_center,
          new_rx,
          new_ry,
          new_phi.cos(),
          new_phi.sin(),
          *theta_start,
          *theta_delta,
        ))
      }
    }
  }

  fn sample_by_length(&self, length: f32) -> Vec2 {
    match self {
      PathSegment::Line {
        start,
        end,
        length: seg_len,
      } => {
        if *seg_len <= LENGTH_EPSILON {
          return *end;
        }
        let t = (length / *seg_len).clamp(0.0, 1.0);
        *start + (*end - *start) * t
      }
      PathSegment::Quadratic {
        start,
        ctrl,
        end,
        table,
      } => {
        let t = table.param_for_length(length);
        quadratic_bezier(*start, *ctrl, *end, t)
      }
      PathSegment::Cubic {
        start,
        ctrl1,
        ctrl2,
        end,
        table,
      } => {
        let t = table.param_for_length(length);
        cubic_bezier(*start, *ctrl1, *ctrl2, *end, t)
      }
      PathSegment::Arc {
        center,
        rx,
        ry,
        cos_phi,
        sin_phi,
        theta_start,
        theta_delta,
        table,
        ..
      } => {
        let t = table.param_for_length(length);
        arc_point(
          *center,
          *rx,
          *ry,
          *cos_phi,
          *sin_phi,
          *theta_start,
          *theta_delta,
          t,
        )
      }
    }
  }
}

/// Returns the analytic 2D AABB of a quadratic bezier with the given control points.
///
/// Solves dB/dt = 0 per component for the interior extrema, evaluates the curve at those
/// in-range t values, and combines with the endpoints. Exact modulo floating-point rounding.
pub(crate) fn quadratic_bezier_aabb(p0: Vec2, p1: Vec2, p2: Vec2) -> (Vec2, Vec2) {
  let mut min = Vec2::new(p0.x.min(p2.x), p0.y.min(p2.y));
  let mut max = Vec2::new(p0.x.max(p2.x), p0.y.max(p2.y));
  for axis in 0..2 {
    let a0 = p0[axis];
    let a1 = p1[axis];
    let a2 = p2[axis];
    let denom = a0 - 2.0 * a1 + a2;
    if denom.abs() <= 1e-12 {
      continue;
    }
    let t = (a0 - a1) / denom;
    if (0.0..=1.0).contains(&t) {
      let v = quadratic_bezier(p0, p1, p2, t)[axis];
      min[axis] = min[axis].min(v);
      max[axis] = max[axis].max(v);
    }
  }
  (min, max)
}

/// Returns the analytic 2D AABB of a cubic bezier with the given control points.
///
/// Solves the quadratic dB/dt = 0 per component for interior extrema (up to two per axis),
/// evaluates the curve at those in-range t values, and combines with the endpoints. Exact
/// modulo floating-point rounding.
pub(crate) fn cubic_bezier_aabb(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> (Vec2, Vec2) {
  let mut min = Vec2::new(p0.x.min(p3.x), p0.y.min(p3.y));
  let mut max = Vec2::new(p0.x.max(p3.x), p0.y.max(p3.y));
  for axis in 0..2 {
    let a0 = p0[axis];
    let a1 = p1[axis];
    let a2 = p2[axis];
    let a3 = p3[axis];
    // B'(t) / 3 = (1-t)^2 (a1-a0) + 2(1-t)t (a2-a1) + t^2 (a3-a2)
    //           = a t^2 + b t + c
    let a = -a0 + 3.0 * a1 - 3.0 * a2 + a3;
    let b = 2.0 * (a0 - 2.0 * a1 + a2);
    let c = a1 - a0;

    let mut consider = |t: f32| {
      if (0.0..=1.0).contains(&t) {
        let v = cubic_bezier(p0, p1, p2, p3, t)[axis];
        min[axis] = min[axis].min(v);
        max[axis] = max[axis].max(v);
      }
    };

    if a.abs() <= 1e-12 {
      if b.abs() > 1e-12 {
        consider(-c / b);
      }
      continue;
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
      continue;
    }
    let sq = disc.sqrt();
    consider((-b + sq) / (2.0 * a));
    consider((-b - sq) / (2.0 * a));
  }
  (min, max)
}

/// Returns the analytic 2D AABB of the elliptical arc covered by the given parametrization.
///
/// `theta_delta` may be positive or negative (signed sweep). Critical angles where
/// dx/dθ = 0 or dy/dθ = 0 are computed analytically; each is shifted by multiples of 2π
/// to test inclusion in the swept range, and contributing points are folded into the
/// endpoint bounds. Exact modulo floating-point rounding.
pub(crate) fn arc_aabb(
  center: Vec2,
  rx: f32,
  ry: f32,
  cos_phi: f32,
  sin_phi: f32,
  theta_start: f32,
  theta_delta: f32,
) -> (Vec2, Vec2) {
  let start = arc_point(
    center,
    rx,
    ry,
    cos_phi,
    sin_phi,
    theta_start,
    theta_delta,
    0.0,
  );
  let end = arc_point(
    center,
    rx,
    ry,
    cos_phi,
    sin_phi,
    theta_start,
    theta_delta,
    1.0,
  );
  let mut min = Vec2::new(start.x.min(end.x), start.y.min(end.y));
  let mut max = Vec2::new(start.x.max(end.x), start.y.max(end.y));

  if theta_delta.abs() <= 1e-12 || rx <= 0.0 || ry <= 0.0 {
    return (min, max);
  }

  let theta_x = (-ry * sin_phi).atan2(rx * cos_phi);
  let theta_y = (ry * cos_phi).atan2(rx * sin_phi);

  let two_pi = std::f32::consts::TAU;
  let theta_end = theta_start + theta_delta;
  let (theta_lo, theta_hi) = if theta_delta >= 0.0 {
    (theta_start, theta_end)
  } else {
    (theta_end, theta_start)
  };

  for theta_c in [
    theta_x,
    theta_x + std::f32::consts::PI,
    theta_y,
    theta_y + std::f32::consts::PI,
  ] {
    // Shift theta_c by 2π·k so it falls into [theta_lo, theta_hi], if possible.
    let k = ((theta_lo - theta_c) / two_pi).ceil();
    let theta_in = theta_c + k * two_pi;
    if theta_in < theta_lo - 1e-6 || theta_in > theta_hi + 1e-6 {
      continue;
    }
    let t = (theta_in - theta_start) / theta_delta;
    if !(-1e-6..=1.0 + 1e-6).contains(&t) {
      continue;
    }
    let p = arc_point(
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      t.clamp(0.0, 1.0),
    );
    min.x = min.x.min(p.x);
    min.y = min.y.min(p.y);
    max.x = max.x.max(p.x);
    max.y = max.y.max(p.y);
  }

  (min, max)
}

fn is_uniform_transform(m: &Matrix3<f32>) -> bool {
  let a = m[(0, 0)];
  let b = m[(0, 1)];
  let c = m[(1, 0)];
  let d = m[(1, 1)];
  let eps = 1e-5;
  // For a scaled rotation: a == d && b == -c
  (a - d).abs() < eps && (b + c).abs() < eps
}

pub(crate) fn transform_segment(
  seg: &PathSegment,
  m: &Matrix3<f32>,
) -> Result<PathSegment, ErrorStack> {
  match seg {
    PathSegment::Line { start, end, .. } => {
      let new_start = apply_transform_to_point(m, *start);
      let new_end = apply_transform_to_point(m, *end);
      let length = (new_end - new_start).norm();
      Ok(PathSegment::Line {
        start: new_start,
        end: new_end,
        length,
      })
    }
    PathSegment::Quadratic {
      start, ctrl, end, ..
    } => {
      let new_start = apply_transform_to_point(m, *start);
      let new_ctrl = apply_transform_to_point(m, *ctrl);
      let new_end = apply_transform_to_point(m, *end);
      let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
        quadratic_bezier(new_start, new_ctrl, new_end, t)
      });
      Ok(PathSegment::Quadratic {
        start: new_start,
        ctrl: new_ctrl,
        end: new_end,
        table,
      })
    }
    PathSegment::Cubic {
      start,
      ctrl1,
      ctrl2,
      end,
      ..
    } => {
      let new_start = apply_transform_to_point(m, *start);
      let new_ctrl1 = apply_transform_to_point(m, *ctrl1);
      let new_ctrl2 = apply_transform_to_point(m, *ctrl2);
      let new_end = apply_transform_to_point(m, *end);
      let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
        cubic_bezier(new_start, new_ctrl1, new_ctrl2, new_end, t)
      });
      Ok(PathSegment::Cubic {
        start: new_start,
        ctrl1: new_ctrl1,
        ctrl2: new_ctrl2,
        end: new_end,
        table,
      })
    }
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
    } => {
      if !is_uniform_transform(m) {
        return Err(ErrorStack::new(
          "apply_transforms: cannot bake a non-uniform transform (e.g. non-uniform scale or skew) \
           into a path that contains arc segments. Arc segments can only be exactly preserved \
           under uniform transforms (translation, rotation, uniform scale). Consider using \
           `path_scale` with a uniform factor, or convert arcs to cubic beziers first.",
        ));
      }

      // Uniform similarity transform: preserves arcs exactly.
      // Extract the uniform scale and rotation angle from the 2x2 block. For the project's
      // row-major rotation matrix `[cos, -sin; sin, cos]`, the signed rotation angle comes
      // from `atan2(m[1,0], m[0,0]) = atan2(sin, cos)`; using `m[0,1]` would yield -angle.
      let cos_a = m[(0, 0)];
      let sin_a = m[(1, 0)];
      let scale = (cos_a * cos_a + sin_a * sin_a).sqrt();
      let rot_angle = sin_a.atan2(cos_a);

      let new_center = apply_transform_to_point(m, *center);
      let new_end = apply_transform_to_point(m, *end);
      let new_rx = rx * scale;
      let new_ry = ry * scale;

      // Compose the rotation into the existing ellipse rotation (phi)
      let old_phi = sin_phi.atan2(*cos_phi);
      let new_phi = old_phi + rot_angle;
      let new_cos_phi = new_phi.cos();
      let new_sin_phi = new_phi.sin();

      let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
        arc_point(
          new_center,
          new_rx,
          new_ry,
          new_cos_phi,
          new_sin_phi,
          *theta_start,
          *theta_delta,
          t,
        )
      });

      Ok(PathSegment::Arc {
        end: new_end,
        center: new_center,
        rx: new_rx,
        ry: new_ry,
        cos_phi: new_cos_phi,
        sin_phi: new_sin_phi,
        theta_start: *theta_start,
        theta_delta: *theta_delta,
        table,
      })
    }
  }
}

#[derive(Clone, Debug)]
pub(crate) struct PathSubpath {
  pub(crate) segments: Vec<PathSegment>,
  pub(crate) cumulative_lengths: Vec<f32>,
  pub(crate) total_length: f32,
  pub(crate) closed: bool,
}

struct SubpathBuilder {
  start: Vec2,
  current: Vec2,
  segments: Vec<PathSegment>,
  closed: bool,
  last_cubic_ctrl: Option<Vec2>,
  last_quad_ctrl: Option<Vec2>,
}

impl SubpathBuilder {
  fn new(start: Vec2) -> Self {
    Self {
      start,
      current: start,
      segments: Vec::new(),
      closed: false,
      last_cubic_ctrl: None,
      last_quad_ctrl: None,
    }
  }
}

impl PathSubpath {
  pub(crate) fn new(segments: Vec<PathSegment>, closed: bool) -> Option<Self> {
    if segments.is_empty() {
      return None;
    }
    let mut cumulative_lengths = Vec::with_capacity(segments.len());
    let mut total_length = 0.0;
    for segment in &segments {
      total_length += segment.length();
      cumulative_lengths.push(total_length);
    }
    if total_length <= LENGTH_EPSILON {
      return None;
    }
    Some(Self {
      segments,
      cumulative_lengths,
      total_length,
      closed,
    })
  }

  fn sample_by_length(&self, length: f32) -> Vec2 {
    let mut idx = match self
      .cumulative_lengths
      .binary_search_by(|len| len.partial_cmp(&length).unwrap_or(Ordering::Less))
    {
      Ok(ix) => ix,
      Err(ix) => ix,
    };
    if idx >= self.segments.len() {
      idx = self.segments.len() - 1;
    }
    let seg_start_len = if idx == 0 {
      0.0
    } else {
      self.cumulative_lengths[idx - 1]
    };
    let seg = &self.segments[idx];
    let seg_len = seg.length();
    if seg_len <= LENGTH_EPSILON {
      return seg.end();
    }
    let local_len = (length - seg_start_len).clamp(0.0, seg_len);
    seg.sample_by_length(local_len)
  }

  pub(crate) fn critical_t_values(&self) -> Vec<f32> {
    if self.segments.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Vec::new();
    }

    let mut out = Vec::with_capacity(self.cumulative_lengths.len() + 1);
    out.push(0.0);
    for len in &self.cumulative_lengths {
      out.push((len / self.total_length).clamp(0.0, 1.0));
    }
    out
  }

  pub(crate) fn segment_intervals(&self) -> Vec<SegmentInterval> {
    if self.segments.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Vec::new();
    }

    let mut intervals = Vec::with_capacity(self.segments.len());
    for (seg, cum_len) in self.segments.iter().zip(self.cumulative_lengths.iter()) {
      let end = (cum_len / self.total_length).clamp(0., 1.);
      intervals.push(SegmentInterval {
        end,
        has_detail: seg.has_detail(),
      });
    }

    intervals
  }

  pub(crate) fn is_closed(&self) -> bool {
    self.closed
  }

  pub(crate) fn total_length(&self) -> f32 {
    self.total_length
  }
}

/// Returns the exact AABB of all segments across the given subpaths, with the given affine
/// transform applied to each segment first. `None` if there are no contributing segments.
///
/// Errors only when the transform is non-uniform (skew or non-uniform scale) and at least
/// one segment is an `Arc`, since the transformed shape is then a conic with no closed-form
/// axis-aligned bound.
pub(crate) fn subpaths_aabb(
  subpaths: &[PathSubpath],
  transform: &Matrix3<f32>,
) -> Result<Option<(Vec2, Vec2)>, ErrorStack> {
  let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
  let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
  let mut any = false;
  for sp in subpaths {
    for seg in &sp.segments {
      let (smin, smax) = seg.aabb_under_transform(transform)?;
      min.x = min.x.min(smin.x);
      min.y = min.y.min(smin.y);
      max.x = max.x.max(smax.x);
      max.y = max.y.max(smax.y);
      any = true;
    }
  }
  Ok(if any { Some((min, max)) } else { None })
}

#[derive(Debug)]
pub struct PathTracerCallable {
  pub interned_t_kwarg: Sym,
  pub subpaths: Rc<Vec<PathSubpath>>,
  pub subpath_cumulative_lengths: Rc<Vec<f32>>,
  pub total_length: f32,
  pub reverse: bool,
  pub override_critical_points: Option<Vec<f32>>,
  pub fill_rule: Option<FillRule>,
  pub transform: Matrix3<f32>,
  pub inward_flip_cache: RefCell<Option<Vec<bool>>>,
}

impl PathTracerCallable {
  pub fn new(
    closed: bool,
    center: bool,
    reverse: bool,
    draw_cmds: Vec<DrawCommand>,
    interned_t_kwarg: Sym,
  ) -> Self {
    let mut subpaths: Vec<PathSubpath> = Vec::new();
    let mut builder: Option<SubpathBuilder> = None;

    fn finalize_subpath(
      builder: &mut Option<SubpathBuilder>,
      force_close: bool,
      out: &mut Vec<PathSubpath>,
    ) {
      let Some(mut builder) = builder.take() else {
        return;
      };

      if force_close && !builder.closed {
        let cur = builder.current;
        let start = builder.start;
        let length = (start - cur).norm();
        if length > LENGTH_EPSILON {
          builder.segments.push(PathSegment::Line {
            start: cur,
            end: start,
            length,
          });
        }
        builder.current = start;
        builder.closed = true;
      }

      if let Some(subpath) = PathSubpath::new(builder.segments, builder.closed) {
        out.push(subpath);
      }
    }

    fn get_or_create_builder(
      builder: &mut Option<SubpathBuilder>,
      start: Vec2,
    ) -> &mut SubpathBuilder {
      if builder.is_none() {
        *builder = Some(SubpathBuilder::new(start));
      }
      builder.as_mut().unwrap()
    }

    for cmd in draw_cmds {
      match cmd {
        DrawCommand::MoveTo(pos) => {
          finalize_subpath(&mut builder, closed, &mut subpaths);
          builder = Some(SubpathBuilder::new(pos));
        }
        DrawCommand::LineTo(pos) => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0.0, 0.0));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let length = (pos - start).norm();
          if length > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Line {
              start,
              end: pos,
              length,
            });
          }
          builder.current = pos;
          builder.last_cubic_ctrl = None;
          builder.last_quad_ctrl = None;
        }
        DrawCommand::QuadraticBezier { ctrl, to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0.0, 0.0));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            quadratic_bezier(start, ctrl, to, t)
          });
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Quadratic {
              start,
              ctrl,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_quad_ctrl = Some(ctrl);
          builder.last_cubic_ctrl = None;
        }
        DrawCommand::SmoothQuadraticBezier { to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0., 0.));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let ctrl = match builder.last_quad_ctrl {
            Some(last_ctrl) => start + (start - last_ctrl),
            None => start,
          };
          let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            quadratic_bezier(start, ctrl, to, t)
          });
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Quadratic {
              start,
              ctrl,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_quad_ctrl = Some(ctrl);
          builder.last_cubic_ctrl = None;
        }
        DrawCommand::CubicBezier { ctrl1, ctrl2, to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0., 0.));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            cubic_bezier(start, ctrl1, ctrl2, to, t)
          });
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Cubic {
              start,
              ctrl1,
              ctrl2,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_cubic_ctrl = Some(ctrl2);
          builder.last_quad_ctrl = None;
        }
        DrawCommand::SmoothCubicBezier { ctrl2, to } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0., 0.));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          let ctrl1 = match builder.last_cubic_ctrl {
            Some(last_ctrl) => start + (start - last_ctrl),
            None => start,
          };
          let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            cubic_bezier(start, ctrl1, ctrl2, to, t)
          });
          if table.total() > LENGTH_EPSILON {
            builder.segments.push(PathSegment::Cubic {
              start,
              ctrl1,
              ctrl2,
              end: to,
              table,
            });
          }
          builder.current = to;
          builder.last_cubic_ctrl = Some(ctrl2);
          builder.last_quad_ctrl = None;
        }
        DrawCommand::Arc {
          rx,
          ry,
          x_axis_rotation,
          large_arc,
          sweep,
          to,
        } => {
          let start = builder
            .as_ref()
            .map(|b| b.current)
            .unwrap_or_else(|| Vec2::new(0.0, 0.0));
          let builder = get_or_create_builder(&mut builder, start);
          builder.closed = false;
          if let Some(segment) =
            build_arc_segment(start, to, rx, ry, x_axis_rotation, large_arc, sweep)
          {
            if segment.length() > LENGTH_EPSILON {
              builder.segments.push(segment);
            }
          }
          builder.current = to;
          builder.last_cubic_ctrl = None;
          builder.last_quad_ctrl = None;
        }
        DrawCommand::Circle {
          center,
          radius,
          reversed,
        } => {
          // Emit as: move to right of circle, two semicircular arcs, close.
          finalize_subpath(&mut builder, closed, &mut subpaths);

          let right = center + Vec2::new(radius, 0.0);
          let left = center - Vec2::new(radius, 0.0);
          // sweep=true here means CCW under math Y-up (right -> top -> left -> bottom -> right).
          // Flipping sweep walks the same start/end points in CW order.
          let sweep = !reversed;

          builder = Some(SubpathBuilder::new(right));
          let b = builder.as_mut().unwrap();

          if let Some(seg) = build_arc_segment(right, left, radius, radius, 0.0, false, sweep) {
            if seg.length() > LENGTH_EPSILON {
              b.segments.push(seg);
            }
          }
          b.current = left;

          if let Some(seg) = build_arc_segment(left, right, radius, radius, 0.0, false, sweep) {
            if seg.length() > LENGTH_EPSILON {
              b.segments.push(seg);
            }
          }
          b.current = right;
          b.closed = true;
          b.last_cubic_ctrl = None;
          b.last_quad_ctrl = None;
        }
        DrawCommand::Rect {
          center,
          width,
          height,
          reversed,
        } => {
          finalize_subpath(&mut builder, closed, &mut subpaths);

          let hw = width * 0.5;
          let hh = height * 0.5;
          let tr = Vec2::new(center.x + hw, center.y + hh);
          let tl = Vec2::new(center.x - hw, center.y + hh);
          let bl = Vec2::new(center.x - hw, center.y - hh);
          let br = Vec2::new(center.x + hw, center.y - hh);

          // Trace the rectangle CCW (in math Y-up) starting at top-right, mirroring `circle`'s
          // start-on-the-right convention.  Reversed flips to CW with the same start point.
          builder = Some(SubpathBuilder::new(tr));
          let b = builder.as_mut().unwrap();

          let edges: [(Vec2, Vec2); 4] = if reversed {
            [(tr, br), (br, bl), (bl, tl), (tl, tr)]
          } else {
            [(tr, tl), (tl, bl), (bl, br), (br, tr)]
          };
          for (start, end) in edges {
            let length = (end - start).norm();
            if length > LENGTH_EPSILON {
              b.segments.push(PathSegment::Line { start, end, length });
            }
          }
          b.current = tr;
          b.closed = true;
          b.last_cubic_ctrl = None;
          b.last_quad_ctrl = None;
        }
        DrawCommand::Close => {
          if let Some(builder) = builder.as_mut() {
            let cur = builder.current;
            let first = builder.start;
            let length = (first - cur).norm();
            if length > LENGTH_EPSILON {
              builder.segments.push(PathSegment::Line {
                start: cur,
                end: first,
                length,
              });
            }
            builder.current = first;
            builder.closed = true;
            builder.last_cubic_ctrl = None;
            builder.last_quad_ctrl = None;
          }
        }
      }
    }

    finalize_subpath(&mut builder, closed, &mut subpaths);

    if center {
      // Local-space (identity transform) — arcs can't fail this branch.
      if let Ok(Some((cmin, cmax))) = subpaths_aabb(&subpaths, &Matrix3::identity()) {
        let center_pt = (cmin + cmax) * 0.5;
        let offset = -center_pt;
        for subpath in &mut subpaths {
          for segment in &mut subpath.segments {
            segment.translate(offset);
          }
        }
      }
    }

    let mut subpath_cumulative_lengths = Vec::with_capacity(subpaths.len());
    let mut total_length = 0.0;
    for subpath in &subpaths {
      total_length += subpath.total_length;
      subpath_cumulative_lengths.push(total_length);
    }

    Self {
      interned_t_kwarg,
      subpaths: Rc::new(subpaths),
      subpath_cumulative_lengths: Rc::new(subpath_cumulative_lengths),
      total_length,
      reverse,
      override_critical_points: None,
      fill_rule: None,
      transform: Matrix3::identity(),
      inward_flip_cache: RefCell::new(None),
    }
  }

  /// Returns the exact AABB of this tracer's geometry, with its 2D affine transform applied.
  ///
  /// Errors only if the path contains arc segments and the transform is non-uniform (i.e. a
  /// skew or non-uniform scale), in which case the transformed arcs are conics with no
  /// closed-form axis-aligned bound. In that case, bake the transform first with
  /// `apply_transforms` (which converts arcs to cubic beziers) and call again.
  pub fn analytic_aabb(&self) -> Result<Option<(Vec2, Vec2)>, ErrorStack> {
    subpaths_aabb(&self.subpaths, &self.transform)
  }

  #[allow(dead_code)]
  pub fn with_fill_rule(mut self, fill_rule: FillRule) -> Self {
    self.fill_rule = Some(fill_rule);
    self
  }

  #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
  pub fn new_with_critical_points(
    closed: bool,
    center: bool,
    reverse: bool,
    draw_cmds: Vec<DrawCommand>,
    interned_t_kwarg: Sym,
    override_critical_points: Option<Vec<f32>>,
  ) -> Self {
    let mut tracer = Self::new(closed, center, reverse, draw_cmds, interned_t_kwarg);
    tracer.override_critical_points = override_critical_points;
    tracer
  }

  /// "Critical t values" refer to points at which sharp features occur in the path, such as where
  /// two line segments intersect.  These are used to provide extra information when sampling to
  /// avoid aliasing missing these sharp details.
  ///
  /// When `reverse` is enabled, these values are transformed via `1 - t` so that they still
  /// correspond to the same physical points on the path when using the reversed sampling.
  pub(crate) fn critical_t_values(&self) -> Vec<f32> {
    if let Some(ref override_cps) = self.override_critical_points {
      let mut out: Vec<f32> = override_cps
        .iter()
        .copied()
        .map(|t| t.clamp(0.0, 1.0))
        .collect();
      if self.reverse {
        for t in &mut out {
          *t = 1.0 - *t;
        }
        out.reverse();
      }
      return out;
    }

    if self.subpaths.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Vec::new();
    }

    let mut out = Vec::with_capacity(self.subpath_cumulative_lengths.len() + 1);
    out.push(0.0);
    let mut offset = 0.0;
    for subpath in self.subpaths.iter() {
      for len in &subpath.cumulative_lengths {
        out.push(((offset + len) / self.total_length).clamp(0.0, 1.0));
      }
      offset += subpath.total_length;
    }

    if self.reverse {
      // Transform all t values via `1 - t` and reverse the order to maintain sorted order
      for t in &mut out {
        *t = 1.0 - *t;
      }
      out.reverse();
    }

    out
  }

  /// Returns critical t-values in per-subpath `[0, 1]` space for use as `adaptive_sample`
  /// initial boundaries.
  ///
  /// When `override_critical_points` is set on the parent, filters those global t-values to
  /// those within this subpath's global span and normalizes them to `[0, 1]`.  This ensures
  /// sharp corners detected by boolean operations (e.g. rectangle clip edges) are preserved
  /// as mandatory sample points during adaptive resampling.
  ///
  /// Falls back to `[0.0, 1.0]` when no override is present or the span is degenerate.
  pub(crate) fn subpath_local_critical_points(&self, subpath_ix: usize) -> Vec<f32> {
    let global_start = if subpath_ix == 0 {
      0.0f32
    } else {
      self.subpath_cumulative_lengths[subpath_ix - 1] / self.total_length
    };
    let global_end = self.subpath_cumulative_lengths[subpath_ix] / self.total_length;
    let span = global_end - global_start;

    if span <= 0.0 {
      return vec![0.0, 1.0];
    }

    let Some(override_cps) = &self.override_critical_points else {
      return vec![0.0, 1.0];
    };

    // Use raw stored values (forward space) — reversal is handled by the caller when
    // it reverses the output point list, not by flipping t-values here.
    let mut local: Vec<f32> = override_cps
      .iter()
      .copied()
      .filter(|&t| t >= global_start && t <= global_end)
      .map(|t| ((t - global_start) / span).clamp(0.0, 1.0))
      .collect();

    if local.iter().all(|&t| t > 1e-6) {
      local.push(0.0);
    }
    if local.iter().all(|&t| t < 1.0 - 1e-6) {
      local.push(1.0);
    }
    local.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    local
  }

  fn sample(&self, t: f32) -> Result<Vec2, ErrorStack> {
    if self.subpaths.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Err(ErrorStack::new(
        "trace_path path has no drawable segments to sample",
      ));
    }

    // When reverse is true, sample at (1 - t) to walk the path backwards
    let t = if self.reverse { 1.0 - t } else { t };

    let target = t * self.total_length;
    let mut subpath_ix = match self
      .subpath_cumulative_lengths
      .binary_search_by(|len| len.partial_cmp(&target).unwrap_or(Ordering::Less))
    {
      Ok(ix) => ix,
      Err(ix) => ix,
    };
    if subpath_ix >= self.subpaths.len() {
      subpath_ix = self.subpaths.len() - 1;
    }
    let subpath_start_len = if subpath_ix == 0 {
      0.0
    } else {
      self.subpath_cumulative_lengths[subpath_ix - 1]
    };
    let subpath = &self.subpaths[subpath_ix];
    let local_target = (target - subpath_start_len).clamp(0.0, subpath.total_length);
    Ok(subpath.sample_by_length(local_target))
  }

  pub(crate) fn from_subpath(subpath: PathSubpath, interned_t_kwarg: Sym, reverse: bool) -> Self {
    let total_length = subpath.total_length;
    Self {
      interned_t_kwarg,
      subpaths: Rc::new(vec![subpath]),
      subpath_cumulative_lengths: Rc::new(vec![total_length]),
      total_length,
      reverse,
      override_critical_points: None,
      fill_rule: None,
      transform: Matrix3::identity(),
      inward_flip_cache: RefCell::new(None),
    }
  }

  pub(crate) fn from_subpaths(subpaths: Vec<PathSubpath>, interned_t_kwarg: Sym) -> Self {
    let mut subpath_cumulative_lengths = Vec::with_capacity(subpaths.len());
    let mut total_length = 0.0;
    for subpath in &subpaths {
      total_length += subpath.total_length;
      subpath_cumulative_lengths.push(total_length);
    }
    Self {
      interned_t_kwarg,
      subpaths: Rc::new(subpaths),
      subpath_cumulative_lengths: Rc::new(subpath_cumulative_lengths),
      total_length,
      reverse: false,
      override_critical_points: None,
      fill_rule: None,
      transform: Matrix3::identity(),
      inward_flip_cache: RefCell::new(None),
    }
  }

  /// Returns whether the left-perpendicular of the tangent in the subpath at `subpath_ix` should
  /// be flipped to point inward. Returns `false` for open or degenerate subpaths. The result is
  /// computed once per subpath via shoelace on a sampled polyline (using `self.transform`) and
  /// cached on the tracer.
  pub(crate) fn subpath_inward_flip(&self, subpath_ix: usize) -> bool {
    let mut slot = self.inward_flip_cache.borrow_mut();
    let cache = slot.get_or_insert_with(|| compute_subpath_inward_flips(self));
    cache.get(subpath_ix).copied().unwrap_or(false)
  }
}

fn compute_subpath_inward_flips(tracer: &PathTracerCallable) -> Vec<bool> {
  const ORIENTATION_TOLERANCE_DEG: f32 = 5.0;
  let polylines = match tracer.sample_subpaths(ORIENTATION_TOLERANCE_DEG.to_radians()) {
    Some(p) => p,
    None => return vec![false; tracer.subpaths.len()],
  };
  polylines
    .into_iter()
    .map(|(points, is_closed)| {
      if !is_closed || points.len() < 3 {
        return false;
      }
      // Shoelace formula. Negative signed area = CW: left-perp `(-y, x)` of the tangent points
      // outward, so flip it to point inward.
      let n = points.len();
      let mut sum = 0.0f32;
      for i in 0..n {
        let p = points[i];
        let q = points[(i + 1) % n];
        sum += p.x * q.y - q.x * p.y;
      }
      sum < 0.0
    })
    .collect()
}

/// Builds tagged-dict `Value::Map` representations of every segment in a path tracer,
/// in subpath order with the path's transform applied.
///
/// Returns one dict per `PathSegment`. Common fields: `type` (line/quad/cubic/arc),
/// `start`, `end`, `length`, `subpath`, `closed`, `t_start`, `t_end` (subpath-local arc-length
/// parameters in [0, 1]), `t_start_global`, `t_end_global` (across the full path).
/// Curve variants add their respective control points / arc parameters.
///
/// The `reverse` flag on the tracer is intentionally not honoured: it affects sampling order
/// but not underlying geometry, which is what callers introspect.
pub(crate) fn build_segment_dicts(tracer: &PathTracerCallable) -> Result<Vec<Value>, ErrorStack> {
  let identity_transform = tracer.transform == Matrix3::identity();
  let global_total = tracer.total_length;
  let total_segments: usize = tracer.subpaths.iter().map(|s| s.segments.len()).sum();
  let mut out = Vec::with_capacity(total_segments);

  for (subpath_ix, subpath) in tracer.subpaths.iter().enumerate() {
    let global_offset = if subpath_ix == 0 {
      0.0
    } else {
      tracer.subpath_cumulative_lengths[subpath_ix - 1]
    };
    let local_total = subpath.total_length;

    for (seg_ix, seg) in subpath.segments.iter().enumerate() {
      let local_prev = if seg_ix == 0 {
        0.0
      } else {
        subpath.cumulative_lengths[seg_ix - 1]
      };
      let local_curr = subpath.cumulative_lengths[seg_ix];

      let meta = SegmentMeta {
        subpath_ix,
        closed: subpath.closed,
        t_start_local: if local_total > 0.0 {
          local_prev / local_total
        } else {
          0.0
        },
        t_end_local: if local_total > 0.0 {
          local_curr / local_total
        } else {
          1.0
        },
        t_start_global: if global_total > 0.0 {
          (global_offset + local_prev) / global_total
        } else {
          0.0
        },
        t_end_global: if global_total > 0.0 {
          (global_offset + local_curr) / global_total
        } else {
          1.0
        },
      };

      let transformed_holder;
      let seg_ref: &PathSegment = if identity_transform {
        seg
      } else {
        transformed_holder = transform_segment(seg, &tracer.transform)
          .map_err(|e| e.wrap("path_segments: failed to apply path transform to a segment"))?;
        &transformed_holder
      };

      out.push(segment_to_dict(seg_ref, meta));
    }
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

  crate::builtins::make_tagged_map(&entries)
}

/// A lazy sequence that yields new `PathTracerCallable`` instances for each subpath
pub(crate) struct SubpathsSeq {
  tracer: Rc<Callable>,
}

impl std::fmt::Debug for SubpathsSeq {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "SubpathsSeq {{ {} subpaths }}",
      self.get_subpaths().map(|s| s.len()).unwrap_or(0)
    )
  }
}

impl SubpathsSeq {
  fn get_tracer(&self) -> Result<&PathTracerCallable, ErrorStack> {
    if let Callable::Dynamic { inner, .. } = self.tracer.as_ref() {
      if let Some(tracer) = inner.as_any().downcast_ref::<PathTracerCallable>() {
        return Ok(tracer);
      }
    }
    Err(ErrorStack::new(
      "Internal error: `SubpathsSeq` constructed with non-`PathTracerCallable` tracer",
    ))
  }

  fn get_subpaths(&self) -> Result<&[PathSubpath], ErrorStack> {
    let tracer = self.get_tracer()?;
    Ok(&tracer.subpaths)
  }

  pub fn new(tracer: Rc<Callable>) -> Self {
    Self { tracer }
  }
}

impl Sequence for SubpathsSeq {
  fn consume<'a>(
    &self,
    _ctx: &'a EvalCtx,
  ) -> Box<dyn Iterator<Item = Result<Value, ErrorStack>> + 'a> {
    let tracer = match self.get_tracer() {
      Ok(t) => t,
      Err(e) => {
        return Box::new(std::iter::once(Err(e)));
      }
    };
    let interned_t_kwarg = tracer.interned_t_kwarg;
    let reverse = tracer.reverse;
    let transform = tracer.transform;
    let subpaths_rc = tracer.subpaths.clone();
    let len = subpaths_rc.len();

    Box::new((0..len).map(move |ix| {
      let subpath = subpaths_rc[ix].clone();
      let mut child = PathTracerCallable::from_subpath(subpath, interned_t_kwarg, reverse);
      child.transform = transform;
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "trace_path".to_owned(),
        inner: Box::new(child),
      })))
    }))
  }
}

fn angle_between(a: Vec2, b: Vec2) -> f32 {
  let la = a.norm();
  let lb = b.norm();
  if la <= LENGTH_EPSILON || lb <= LENGTH_EPSILON {
    return 0.0;
  }
  let cos = (a.dot(&b) / (la * lb)).clamp(-1.0, 1.0);
  cos.acos()
}

fn segment_turning_angle(seg: &PathSegment) -> f32 {
  match seg {
    PathSegment::Line { .. } => 0.0,
    PathSegment::Quadratic {
      start, ctrl, end, ..
    } => angle_between(*ctrl - *start, *end - *ctrl),
    PathSegment::Cubic {
      start,
      ctrl1,
      ctrl2,
      end,
      ..
    } => {
      let a = angle_between(*ctrl1 - *start, *ctrl2 - *ctrl1);
      let b = angle_between(*ctrl2 - *ctrl1, *end - *ctrl2);
      a + b
    }
    PathSegment::Arc { theta_delta, .. } => theta_delta.abs(),
  }
}

fn segment_subdivisions(seg: &PathSegment, angle_tolerance: f32) -> usize {
  if angle_tolerance <= 0.0 || !seg.has_detail() {
    return 1;
  }
  let angle = segment_turning_angle(seg);
  if angle <= 0.0 {
    return 1;
  }
  let count = (angle / angle_tolerance).ceil() as usize;
  count.max(1)
}

pub(crate) fn sample_subpath_points(
  subpath: &PathSubpath,
  angle_tolerance: f32,
  include_end: bool,
) -> Vec<Vec2> {
  if subpath.segments.is_empty() || subpath.total_length <= LENGTH_EPSILON {
    return Vec::new();
  }

  let mut extra = 0usize;
  for seg in &subpath.segments {
    extra = extra.saturating_add(segment_subdivisions(seg, angle_tolerance).saturating_sub(1));
  }

  let base_count = if include_end {
    subpath.segments.len() + 1
  } else {
    subpath.segments.len()
  };
  let target_count = base_count.saturating_add(extra);

  let guides = subpath.critical_t_values();
  let intervals = subpath.segment_intervals();
  let interval_weights = build_interval_weights(&guides, &[intervals]);
  let t_samples = build_topology_samples(
    target_count,
    Some(&guides),
    interval_weights.as_deref(),
    include_end,
  );

  t_samples
    .into_iter()
    .map(|t| subpath.sample_by_length(t * subpath.total_length()))
    .collect()
}

impl PathSampler for PathTracerCallable {
  fn critical_t_values(&self) -> Vec<f32> {
    PathTracerCallable::critical_t_values(self)
  }

  fn subpath_topology(&self) -> Option<Vec<SubpathTopology>> {
    Some(
      self
        .subpaths
        .iter()
        .map(|sp| SubpathTopology {
          closed: sp.closed,
          segment_count: sp.segments.len(),
        })
        .collect(),
    )
  }

  fn fill_rule(&self) -> Option<FillRule> {
    self.fill_rule
  }

  fn to_lyon_path_for_tessellation(&self) -> Option<lyon_tessellation::path::Path> {
    use lyon_tessellation::{
      geom::{Angle, Arc, Point, Vector},
      path::Path,
    };

    let m = &self.transform;
    // Apply the 2D affine transform stored in self.transform to a Vec2.
    let tx = |pt: Vec2| -> Point<f32> {
      let v = m * nalgebra::Vector3::new(pt.x, pt.y, 1.0);
      Point::new(v.x, v.y)
    };

    let mut builder = Path::builder();

    for subpath in self.subpaths.iter() {
      if subpath.segments.is_empty() {
        continue;
      }

      builder.begin(tx(subpath.segments[0].start_point()));

      for seg in &subpath.segments {
        match seg {
          PathSegment::Line { end, .. } => {
            builder.line_to(tx(*end));
          }
          PathSegment::Quadratic { ctrl, end, .. } => {
            builder.quadratic_bezier_to(tx(*ctrl), tx(*end));
          }
          PathSegment::Cubic {
            ctrl1, ctrl2, end, ..
          } => {
            builder.cubic_bezier_to(tx(*ctrl1), tx(*ctrl2), tx(*end));
          }
          PathSegment::Arc {
            center,
            rx,
            ry,
            cos_phi,
            sin_phi,
            theta_start,
            theta_delta,
            ..
          } => {
            // Convert center-parametric arc to cubic bezier approximations in
            // untransformed space, then apply the affine transform to the control points.
            // Affine transforms distribute over bezier interpolation, so this is exact
            // even for non-uniform scale and shear.
            let arc = Arc {
              center: Point::new(center.x, center.y),
              radii: Vector::new(*rx, *ry),
              start_angle: Angle::radians(*theta_start),
              sweep_angle: Angle::radians(*theta_delta),
              x_rotation: Angle::radians(sin_phi.atan2(*cos_phi)),
            };
            arc.for_each_cubic_bezier(&mut |seg| {
              builder.cubic_bezier_to(
                tx(Vec2::new(seg.ctrl1.x, seg.ctrl1.y)),
                tx(Vec2::new(seg.ctrl2.x, seg.ctrl2.y)),
                tx(Vec2::new(seg.to.x, seg.to.y)),
              );
            });
          }
        }
      }

      builder.end(subpath.closed);
    }

    Some(builder.build())
  }

  fn transform(&self) -> &Matrix3<f32> {
    &self.transform
  }

  fn with_transform(&self, t: Matrix3<f32>) -> Box<dyn DynamicCallable> {
    Box::new(PathTracerCallable {
      interned_t_kwarg: self.interned_t_kwarg,
      subpaths: self.subpaths.clone(),
      subpath_cumulative_lengths: self.subpath_cumulative_lengths.clone(),
      total_length: self.total_length,
      reverse: self.reverse,
      override_critical_points: self.override_critical_points.clone(),
      fill_rule: self.fill_rule,
      transform: t * self.transform,
      inward_flip_cache: RefCell::new(None),
    })
  }

  fn eval_at_raw(&self, t: f32, _ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    self.sample(t)
  }

  fn sample_subpaths(&self, angle_tolerance: f32) -> Option<Vec<(Vec<Vec2>, bool)>> {
    let transform = &self.transform;
    let mut result = Vec::with_capacity(self.subpaths.len());
    for subpath in self.subpaths.iter() {
      let is_closed = subpath.is_closed();
      let include_end = !is_closed;
      let mut points = sample_subpath_points(subpath, angle_tolerance, include_end);
      if self.reverse {
        points.reverse();
      }
      if *transform != Matrix3::identity() {
        for p in &mut points {
          *p = apply_transform_to_point(transform, *p);
        }
      }
      result.push((points, is_closed));
    }
    Some(result)
  }

  fn sample_subpaths_with_limit(
    &self,
    angle_tolerance: f32,
    total_limit: Option<usize>,
  ) -> Option<Vec<(Vec<Vec2>, bool)>> {
    use crate::mesh_ops::adaptive_sampler::{
      adaptive_sample, distribute_samples_by_mass, recommended_n_dense, DEFAULT_MIN_SEGMENT_LENGTH,
    };

    let limit = match total_limit {
      None => return self.sample_subpaths(angle_tolerance),
      Some(l) => l,
    };

    // Compute natural samples to check whether reduction is actually needed.
    let natural = self.sample_subpaths(angle_tolerance)?;
    let natural_total: usize = natural.iter().map(|(pts, _)| pts.len()).sum();

    if limit >= natural_total {
      return Some(natural);
    }

    // Adaptive reduction: distribute `limit` points across subpaths proportionally by
    // curvature+arc-length mass, preserving detected sharp corners as mandatory boundaries.
    let n_subpaths = self.subpaths.len();

    // One mandatory sample per subpath (the t=0 start, always included by adaptive_sample).
    let free_budget = limit.saturating_sub(n_subpaths);
    let n_dense = recommended_n_dense(limit, n_subpaths);

    let allocations = {
      let samplers: Vec<_> = self
        .subpaths
        .iter()
        .map(|sp| {
          let len = sp.total_length;
          move |t: f32| sp.sample_by_length(t * len)
        })
        .collect();
      distribute_samples_by_mass::<Vec2, _>(free_budget, &samplers, n_dense)
    };

    let transform = &self.transform;
    let mut result = Vec::with_capacity(n_subpaths);

    for (subpath_ix, (subpath, &extra)) in self.subpaths.iter().zip(allocations.iter()).enumerate()
    {
      let budget = 1 + extra;
      let local_cps = self.subpath_local_critical_points(subpath_ix);
      let len = subpath.total_length;

      let t_samples = adaptive_sample::<Vec2, _>(
        budget,
        &local_cps,
        |t| subpath.sample_by_length(t * len),
        DEFAULT_MIN_SEGMENT_LENGTH,
      );

      let mut points: Vec<Vec2> = t_samples
        .iter()
        .map(|&t| subpath.sample_by_length(t * len))
        .collect();

      if self.reverse {
        points.reverse();
      }
      if *transform != Matrix3::identity() {
        for p in &mut points {
          *p = apply_transform_to_point(transform, *p);
        }
      }

      result.push((points, subpath.is_closed()));
    }

    Some(result)
  }
}

/// Converts a tagged dict (built by the `path_*` constructor builtins, e.g. `path_move`,
/// `path_line`, `path_close`) into a `DrawCommand`.
///
/// The dict shapes are documented alongside the constructor builtins in `builtins.rs`.
/// While users could in principle hand-construct these maps, that is not a supported
/// public API and the function therefore emits clear errors when fields are missing or
/// have the wrong type.
pub(crate) fn map_to_draw_command(
  map: &FxHashMap<String, Value>,
) -> Result<DrawCommand, ErrorStack> {
  let kind = map
    .get("type")
    .and_then(|v| v.as_str())
    .ok_or_else(|| ErrorStack::new("draw command map missing string `type` field"))?;

  fn get_vec2(map: &FxHashMap<String, Value>, key: &str) -> Result<Vec2, ErrorStack> {
    map
      .get(key)
      .and_then(|v| v.as_vec2().copied())
      .ok_or_else(|| ErrorStack::new(format!("draw command map missing vec2 field `{key}`")))
  }
  fn get_float(map: &FxHashMap<String, Value>, key: &str) -> Result<f32, ErrorStack> {
    map
      .get(key)
      .and_then(|v| v.as_float())
      .ok_or_else(|| ErrorStack::new(format!("draw command map missing numeric field `{key}`")))
  }
  fn get_bool(map: &FxHashMap<String, Value>, key: &str) -> Result<bool, ErrorStack> {
    map
      .get(key)
      .and_then(|v| v.as_bool())
      .ok_or_else(|| ErrorStack::new(format!("draw command map missing bool field `{key}`")))
  }

  match kind {
    "move" => Ok(DrawCommand::MoveTo(get_vec2(map, "to")?)),
    "line" => Ok(DrawCommand::LineTo(get_vec2(map, "to")?)),
    "quad" => Ok(DrawCommand::QuadraticBezier {
      ctrl: get_vec2(map, "ctrl")?,
      to: get_vec2(map, "to")?,
    }),
    "smooth_quad" => Ok(DrawCommand::SmoothQuadraticBezier {
      to: get_vec2(map, "to")?,
    }),
    "cubic" => Ok(DrawCommand::CubicBezier {
      ctrl1: get_vec2(map, "ctrl1")?,
      ctrl2: get_vec2(map, "ctrl2")?,
      to: get_vec2(map, "to")?,
    }),
    "smooth_cubic" => Ok(DrawCommand::SmoothCubicBezier {
      ctrl2: get_vec2(map, "ctrl2")?,
      to: get_vec2(map, "to")?,
    }),
    "arc" => Ok(DrawCommand::Arc {
      rx: get_float(map, "rx")?,
      ry: get_float(map, "ry")?,
      x_axis_rotation: get_float(map, "x_axis_rotation")?,
      large_arc: get_bool(map, "large_arc")?,
      sweep: get_bool(map, "sweep")?,
      to: get_vec2(map, "to")?,
    }),
    "circle" => Ok(DrawCommand::Circle {
      center: get_vec2(map, "center")?,
      radius: get_float(map, "radius")?,
      reversed: map.get("reversed").and_then(|v| v.as_bool()).unwrap_or(false),
    }),
    "rect" => Ok(DrawCommand::Rect {
      center: get_vec2(map, "center")?,
      width: get_float(map, "width")?,
      height: get_float(map, "height")?,
      reversed: map.get("reversed").and_then(|v| v.as_bool()).unwrap_or(false),
    }),
    "close" => Ok(DrawCommand::Close),
    other => Err(ErrorStack::new(format!(
      "unknown draw command `type`: \"{other}\".  Expected one of: move, line, quad, smooth_quad, \
       cubic, smooth_cubic, arc, circle, rect, close"
    ))),
  }
}

pub fn build_path_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let cmds_seq = arg_refs[0].resolve(args, kwargs).as_sequence().unwrap();
  let closed = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
  let center = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
  let reverse = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
  let fill_rule_val = arg_refs[4].resolve(args, kwargs);
  let fill_rule = match fill_rule_val {
    Value::Nil => None,
    val => Some(FillRule::parse(val, "build_path")?),
  };

  let mut draw_cmds: Vec<DrawCommand> = Vec::new();
  for item in cmds_seq.consume(ctx) {
    let val = item?;
    match val {
      Value::Map(map) => {
        let cmd = map_to_draw_command(&map).map_err(|err| {
          err.wrap("Error converting sequence item to draw command in `build_path`")
        })?;
        draw_cmds.push(cmd);
      }
      other => {
        return Err(ErrorStack::new(format!(
          "build_path: expected a sequence of draw command maps (built via `path_move`, \
           `path_line`, etc., or via the `path {{ ... }}` macro). Found a non-map item: {other:?}"
        )));
      }
    }
  }

  let interned_t_kwarg = ctx.interned_symbols.intern("t");
  let mut path_tracer =
    PathTracerCallable::new(closed, center, reverse, draw_cmds, interned_t_kwarg);
  path_tracer.fill_rule = fill_rule;
  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: "trace_path".to_owned(),
    inner: Box::new(path_tracer),
  })))
}

pub fn discretize_path_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let path_val = arg_refs[0].resolve(args, kwargs);
  let path_callable = path_val.as_callable().ok_or_else(|| {
    ErrorStack::new(format!(
      "discretize_path: expected a path callable, found: {path_val:?}"
    ))
  })?;

  let curve_angle_degrees = arg_refs[1].resolve(args, kwargs).as_float().unwrap() as f64;
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

  let closed_override_val = arg_refs[3].resolve(args, kwargs);
  let closed_override = match closed_override_val {
    Value::Bool(b) => Some(*b),
    Value::Nil => None,
    _ => {
      return Err(ErrorStack::new(format!(
        "Invalid closed argument for `discretize_path`; expected bool or nil, found: \
         {closed_override_val:?}"
      )))
    }
  };

  let subpaths = sample_path_subpaths(
    ctx,
    path_callable,
    curve_angle_radians,
    sample_count,
    closed_override,
    "discretize_path",
  )?;

  let draw_cmds = polylines_to_draw_commands(subpaths.into_iter());
  let interned_t_kwarg = ctx.interned_symbols.intern("t");
  let tracer = PathTracerCallable::new(false, false, false, draw_cmds, interned_t_kwarg);
  Ok(Value::Callable(Rc::new(Callable::Dynamic {
    name: "discretize_path".to_owned(),
    inner: Box::new(tracer),
  })))
}

#[derive(Clone, Debug)]
pub enum DrawCommand {
  MoveTo(Vec2),
  LineTo(Vec2),
  QuadraticBezier {
    ctrl: Vec2,
    to: Vec2,
  },
  SmoothQuadraticBezier {
    to: Vec2,
  },
  CubicBezier {
    ctrl1: Vec2,
    ctrl2: Vec2,
    to: Vec2,
  },
  SmoothCubicBezier {
    ctrl2: Vec2,
    to: Vec2,
  },
  Arc {
    rx: f32,
    ry: f32,
    x_axis_rotation: f32,
    large_arc: bool,
    sweep: bool,
    to: Vec2,
  },
  Circle {
    center: Vec2,
    radius: f32,
    reversed: bool,
  },
  Rect {
    center: Vec2,
    width: f32,
    height: f32,
    reversed: bool,
  },
  Close,
}

fn quadratic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, t: f32) -> Vec2 {
  let u = 1.0 - t;
  let tt = t * t;
  let uu = u * u;
  uu * p0 + 2.0 * u * t * p1 + tt * p2
}

fn cubic_bezier(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2, t: f32) -> Vec2 {
  let u = 1.0 - t;
  let tt = t * t;
  let uu = u * u;
  let uuu = uu * u;
  let ttt = tt * t;
  uuu * p0 + 3.0 * uu * t * p1 + 3.0 * u * tt * p2 + ttt * p3
}

fn arc_point(
  center: Vec2,
  rx: f32,
  ry: f32,
  cos_phi: f32,
  sin_phi: f32,
  theta_start: f32,
  theta_delta: f32,
  t: f32,
) -> Vec2 {
  let theta = theta_start + theta_delta * t;
  let (sin_theta, cos_theta) = theta.sin_cos();
  let x = rx * cos_theta;
  let y = ry * sin_theta;
  let px = cos_phi * x - sin_phi * y + center.x;
  let py = sin_phi * x + cos_phi * y + center.y;
  Vec2::new(px, py)
}

fn build_arc_segment(
  start: Vec2,
  end: Vec2,
  rx: f32,
  ry: f32,
  x_axis_rotation: f32,
  large_arc: bool,
  sweep: bool,
) -> Option<PathSegment> {
  let mut rx = rx.abs();
  let mut ry = ry.abs();
  if rx <= LENGTH_EPSILON || ry <= LENGTH_EPSILON {
    let length = (end - start).norm();
    return Some(PathSegment::Line { start, end, length });
  }

  if (end - start).norm() <= LENGTH_EPSILON {
    return None;
  }

  let phi = x_axis_rotation.to_radians();
  let cos_phi = phi.cos();
  let sin_phi = phi.sin();
  let dx = (start.x - end.x) / 2.0;
  let dy = (start.y - end.y) / 2.0;
  let x1p = cos_phi * dx + sin_phi * dy;
  let y1p = -sin_phi * dx + cos_phi * dy;

  let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
  if lambda > 1.0 {
    let scale = lambda.sqrt();
    rx *= scale;
    ry *= scale;
  }

  let rx_sq = rx * rx;
  let ry_sq = ry * ry;
  let x1p_sq = x1p * x1p;
  let y1p_sq = y1p * y1p;
  let denom = rx_sq * y1p_sq + ry_sq * x1p_sq;
  if denom.abs() <= LENGTH_EPSILON {
    let length = (end - start).norm();
    return Some(PathSegment::Line { start, end, length });
  }

  let numerator = rx_sq * ry_sq - rx_sq * y1p_sq - ry_sq * x1p_sq;
  let coef = (numerator / denom).max(0.).sqrt();
  let sign = if large_arc == sweep { -1. } else { 1. };
  let coef = sign * coef;

  let cxp = coef * (rx * y1p / ry);
  let cyp = coef * (-ry * x1p / rx);
  let cx = cos_phi * cxp - sin_phi * cyp + (start.x + end.x) / 2.;
  let cy = sin_phi * cxp + cos_phi * cyp + (start.y + end.y) / 2.;
  let center = Vec2::new(cx, cy);

  let v1 = Vec2::new((x1p - cxp) / rx, (y1p - cyp) / ry);
  let v2 = Vec2::new((-x1p - cxp) / rx, (-y1p - cyp) / ry);
  let theta_start = v1.y.atan2(v1.x);
  let mut theta_delta = (v1.x * v2.y - v1.y * v2.x).atan2(v1.x * v2.x + v1.y * v2.y);

  if !sweep && theta_delta > 0. {
    theta_delta -= 2. * PI;
  } else if sweep && theta_delta < 0. {
    theta_delta += 2. * PI;
  }

  let table = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
    arc_point(
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      t,
    )
  });

  Some(PathSegment::Arc {
    end,
    center,
    rx,
    ry,
    cos_phi,
    sin_phi,
    theta_start,
    theta_delta,
    table,
  })
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
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let svg_path_str = arg_refs[0].resolve(args, kwargs).as_str().unwrap();
      let center = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
      let reverse = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();
      let fill_rule_val = arg_refs[3].resolve(args, kwargs);
      let fill_rule = match fill_rule_val {
        Value::Nil => None,
        val => Some(FillRule::parse(val, "trace_svg_path")?),
      };

      let draw_cmds = parse_svg_path_to_draw_commands(svg_path_str)
        .map_err(|err| err.wrap("Error while parsing SVG path string"))?;

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let mut path_tracer =
        PathTracerCallable::new(false, center, reverse, draw_cmds, interned_t_kwarg);
      path_tracer.fill_rule = fill_rule;
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "trace_svg_path".to_owned(),
        inner: Box::new(path_tracer),
      })))
    }
    _ => unimplemented!(),
  }
}

pub fn text_to_path_impl(
  ctx: &EvalCtx,
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
      let center = arg_refs[6].resolve(args, kwargs).as_bool().unwrap();
      let fill_rule_val = arg_refs[7].resolve(args, kwargs);
      let fill_rule = match fill_rule_val {
        Value::Nil => None,
        val => Some(FillRule::parse(val, "text_to_path")?),
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

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let mut path_tracer =
        PathTracerCallable::new(false, center, false, draw_cmds, interned_t_kwarg);
      path_tracer.fill_rule = fill_rule;

      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "text_to_path".to_owned(),
        inner: Box::new(path_tracer),
      })))
    }
    _ => unimplemented!(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parse_and_eval_program;

  fn assert_vec2_close(actual: Vec2, expected: Vec2) {
    let diff = (actual - expected).norm();
    assert!(
      diff < 1e-4,
      "Expected {expected:?}, got {actual:?} (diff {diff})"
    );
  }

  #[test]
  fn test_path_tracer_line_segments() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 3.0)),
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.25).unwrap(), Vec2::new(1.0, 0.0));
    assert_vec2_close(tracer.sample(0.75).unwrap(), Vec2::new(1.0, 2.0));
  }

  fn assert_aabb_close(actual: (Vec2, Vec2), expected_min: Vec2, expected_max: Vec2) {
    assert_vec2_close(actual.0, expected_min);
    assert_vec2_close(actual.1, expected_max);
  }

  #[test]
  fn test_quadratic_bezier_aabb_interior_extremum() {
    // Symmetric quadratic peaking at y=1 above the chord (0,0)-(2,0).
    let (min, max) = quadratic_bezier_aabb(
      Vec2::new(0.0, 0.0),
      Vec2::new(1.0, 2.0),
      Vec2::new(2.0, 0.0),
    );
    // Peak is at t = 0.5 → B(0.5) = 0.25*(0,0) + 0.5*(1,2) + 0.25*(2,0) = (1, 1).
    assert_vec2_close(min, Vec2::new(0.0, 0.0));
    assert_vec2_close(max, Vec2::new(2.0, 1.0));
  }

  #[test]
  fn test_cubic_bezier_aabb_monotonic() {
    // Monotone-in-x, monotone-in-y cubic — extrema are at the endpoints.
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
    // Full circle of radius 5 centered at origin.
    use std::f32::consts::TAU;
    let (min, max) = arc_aabb(Vec2::new(0.0, 0.0), 5.0, 5.0, 1.0, 0.0, 0.0, TAU);
    assert_vec2_close(min, Vec2::new(-5.0, -5.0));
    assert_vec2_close(max, Vec2::new(5.0, 5.0));
  }

  #[test]
  fn test_arc_aabb_quarter_circle() {
    // Quarter circle from (5,0) to (0,5), sweeping counter-clockwise.
    use std::f32::consts::FRAC_PI_2;
    let (min, max) = arc_aabb(Vec2::new(0.0, 0.0), 5.0, 5.0, 1.0, 0.0, 0.0, FRAC_PI_2);
    assert_vec2_close(min, Vec2::new(0.0, 0.0));
    assert_vec2_close(max, Vec2::new(5.0, 5.0));
  }

  #[test]
  fn test_path_tracer_analytic_aabb_circle() {
    // `Circle` draw command emits two semicircle arcs; the AABB should match the bounding
    // square exactly, not the polyline approximation.
    let cmds = vec![DrawCommand::Circle {
      center: Vec2::new(3.0, -2.0),
      radius: 4.0,
      reversed: false,
    }];
    let tracer = PathTracerCallable::new(true, false, false, cmds, Sym(0));
    let (min, max) = tracer.analytic_aabb().unwrap().unwrap();
    assert_aabb_close((min, max), Vec2::new(-1.0, -6.0), Vec2::new(7.0, 2.0));
  }

  #[test]
  fn test_analytic_aabb_under_rotation_of_elliptical_arc() {
    // A half-ellipse with rx=2, ry=1, sweeping from angle 0 to π. Local-space AABB is
    // [(-2, 0), (2, 1)]. Rotating by +π/2 about the origin should give [(-1, -2), (0, 2)] —
    // a sign error in the rotation extraction (atan2(b, a) instead of atan2(c, a)) would
    // instead bake -π/2, producing [(-1, 0)...(0, 2)] vs the expected.
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let (lmin, lmax) = tracer.analytic_aabb().unwrap().unwrap();
    assert_vec2_close(lmin, Vec2::new(-2.0, 0.0));
    assert_vec2_close(lmax, Vec2::new(2.0, 1.0));

    let (sin_a, cos_a) = FRAC_PI_2.sin_cos();
    let rot = Matrix3::new(cos_a, -sin_a, 0.0, sin_a, cos_a, 0.0, 0.0, 0.0, 1.0);
    let mut rotated = tracer;
    rotated.transform = rot;
    let (rmin, rmax) = rotated.analytic_aabb().unwrap().unwrap();
    assert_vec2_close(rmin, Vec2::new(-1.0, -2.0));
    assert_vec2_close(rmax, Vec2::new(0.0, 2.0));
  }

  #[test]
  fn test_path_tracer_analytic_aabb_under_translation() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 1.0)),
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let translated = Matrix3::new(1.0, 0.0, 10.0, 0.0, 1.0, -5.0, 0.0, 0.0, 1.0);
    let mut tracer_t = tracer;
    tracer_t.transform = translated;
    let (min, max) = tracer_t.analytic_aabb().unwrap().unwrap();
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let guides = tracer.critical_t_values();

    assert_eq!(guides.len(), 3);
    assert!((guides[0] - 0.0).abs() < 1e-6);
    assert!((guides[1] - 0.5).abs() < 1e-6);
    assert!((guides[2] - 1.0).abs() < 1e-6);
  }

  #[test]
  fn test_path_tracer_override_critical_points() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 1.0)),
      DrawCommand::LineTo(Vec2::new(0.0, 1.0)),
      DrawCommand::Close,
    ];
    let override_cps = vec![0.0, 0.25, 0.5, 0.75, 1.0];
    let tracer = PathTracerCallable::new_with_critical_points(
      false,
      false,
      false,
      cmds,
      Sym(0),
      Some(override_cps.clone()),
    );

    assert_eq!(tracer.critical_t_values(), override_cps);
  }

  #[test]
  fn test_path_tracer_multiple_subpaths_sampling() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::MoveTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(11.0, 0.0)),
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.25).unwrap(), Vec2::new(0.5, 0.0));
    assert_vec2_close(tracer.sample(0.75).unwrap(), Vec2::new(10.5, 0.0));
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_eq!(tracer.subpaths.len(), 2);
    assert_eq!(tracer.subpaths[1].segments.len(), 2);
    match &tracer.subpaths[1].segments[1] {
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let subpath = &tracer.subpaths[0];

    let points = sample_subpath_points(subpath, std::f32::consts::FRAC_PI_4, true);
    assert_eq!(points.len(), 3);
    assert_vec2_close(points[0], Vec2::new(0.0, 0.0));
    assert_vec2_close(points[2], Vec2::new(2.0, 0.0));
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
    // 10x10 Box ending at (10, 10). Center is (5, 5).
    // Result should be shifted by (-5, -5), moving (0,0) to (-5, -5).
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 10.0)),
    ];
    let tracer = PathTracerCallable::new(false, true, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(-5.0, -5.0)); // was 0,0
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(5.0, -5.0)); // was 10,0
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(5.0, 5.0)); // was 10,10
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(2.0, 0.0));
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_eq!(tracer.subpaths.len(), 1);
    assert_eq!(tracer.subpaths[0].segments.len(), 2);
    match &tracer.subpaths[0].segments[1] {
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_eq!(tracer.subpaths.len(), 1);
    assert_eq!(tracer.subpaths[0].segments.len(), 2);
    match &tracer.subpaths[0].segments[1] {
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
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(1.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(-1.0, 0.0));
  }

  #[test]
  fn test_path_tracer_closed_flag_adds_closing_segment() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(2.0, 2.0)),
    ];
    let tracer = PathTracerCallable::new(true, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(0.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_absolute_line() {
    // Simple absolute path: move to origin, line to (10, 0), line to (10, 10)
    let svg = "M 0 0 L 10 0 L 10 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(10.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_relative_line() {
    // Relative path: move to (5, 5), relative line +10 in x, then +10 in y
    let svg = "M 5 5 l 10 0 l 0 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(5.0, 5.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(15.0, 5.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(15.0, 15.0));
  }

  #[test]
  fn test_parse_svg_path_horizontal_vertical() {
    // H and V commands
    let svg = "M 0 0 H 10 V 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(10.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_cubic_bezier() {
    // Cubic bezier from (0,0) to (10,0) with control points
    let svg = "M 0 0 C 3 5, 7 5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_quadratic_bezier() {
    // Quadratic bezier from (0,0) to (10,0) with control point at (5, 5)
    let svg = "M 0 0 Q 5 5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_arc() {
    // Arc from (1,0) to (-1,0) with rx=ry=1
    let svg = "M 1 0 A 1 1 0 0 1 -1 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(1.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(-1.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_close() {
    // Triangle that closes back to start
    let svg = "M 0 0 L 10 0 L 5 10 Z";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();

    // Should have MoveTo, LineTo, LineTo, Close
    assert_eq!(cmds.len(), 4);
    assert!(matches!(cmds[3], DrawCommand::Close));

    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    // With close, path goes back to origin
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(0.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_smooth_cubic() {
    // Smooth cubic: S command reflects the previous control point
    let svg = "M 0 0 C 0 5, 5 5, 5 0 S 10 -5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    assert!(matches!(cmds[2], DrawCommand::SmoothCubicBezier { .. }));
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert!(matches!(
      tracer.subpaths[0].segments[1],
      PathSegment::Cubic { .. }
    ));
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_smooth_quadratic() {
    // Smooth quadratic: T command reflects the previous control point
    let svg = "M 0 0 Q 2.5 5, 5 0 T 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    assert!(matches!(cmds[2], DrawCommand::SmoothQuadraticBezier { .. }));
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert!(matches!(
      tracer.subpaths[0].segments[1],
      PathSegment::Quadratic { .. }
    ));
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_path_block_alias_draw_commands() {
    let src = r#"
path = build_path(path {
  move(0, 0)
  quad_bezier(vec2(1, 0), vec2(2, 0))
  smooth_quadratic_bezier(3, 0)
  cubic_bezier(vec2(4, 0), vec2(5, 0), vec2(6, 0))
  smooth_bezier(vec2(7, 0), vec2(8, 0))
})
p0 = path(0)
p1 = path(1)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    let p0_val = ctx.get_global("p0").unwrap();
    let p1_val = ctx.get_global("p1").unwrap();
    let p0 = p0_val.as_vec2().unwrap();
    let p1 = p1_val.as_vec2().unwrap();

    assert_vec2_close(*p0, Vec2::new(0.0, 0.0));
    assert_vec2_close(*p1, Vec2::new(8.0, 0.0));
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
path = build_path(path {
  move(0, 0)
  line(1, 0)
  line(1, 1)
  line(0, 1)
  close()
})
mesh = tessellate_path(path)
"#;

    let ctx = parse_and_eval_program(src).unwrap();
    let mesh_val = ctx.get_global("mesh").unwrap();
    let mesh = mesh_val.as_mesh().unwrap();
    assert_eq!(mesh.mesh.vertices.len(), 4);
    assert_eq!(mesh.mesh.faces.len(), 2);
  }

  #[test]
  fn test_subpaths_builtin() {
    let src = r#"
// Create a path with two disconnected subpaths via two move commands
path = build_path(path {
  move(0, 0)
  line(10, 0)

  move(100, 0)
  line(100, 20)
})

// Extract the subpaths as a sequence
subs = subpaths(path)

// Collect the subpaths into an array and sample them
sub_array = collect(subs)
count = len(sub_array)

// Sample the first subpath
first = sub_array[0]
first_start = first(0)
first_end = first(1)

// Sample the second subpath
second = sub_array[1]
second_start = second(0)
second_end = second(1)
"#;

    let ctx = parse_and_eval_program(src).unwrap();

    // Verify we have 2 subpaths
    let count = ctx.get_global("count").unwrap();
    assert_eq!(count.as_int().unwrap(), 2);

    // Verify first subpath samples correctly
    let first_start = ctx.get_global("first_start").unwrap();
    let first_end = ctx.get_global("first_end").unwrap();
    assert_vec2_close(*first_start.as_vec2().unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(*first_end.as_vec2().unwrap(), Vec2::new(10.0, 0.0));

    // Verify second subpath samples correctly
    let second_start = ctx.get_global("second_start").unwrap();
    let second_end = ctx.get_global("second_end").unwrap();
    assert_vec2_close(*second_start.as_vec2().unwrap(), Vec2::new(100.0, 0.0));
    assert_vec2_close(*second_end.as_vec2().unwrap(), Vec2::new(100.0, 20.0));
  }

  #[test]
  fn test_lerp_paths_midpoint() {
    let src = r#"
path_a = build_path(path {
  move(0, 0)
  line(2, 0)
})
path_b = build_path(path {
  move(0, 2)
  line(2, 2)
})
lerped = lerp_paths(path_a, path_b, 0.5)
result = lerped(0.5)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let result = ctx.get_global("result").unwrap();
    assert_vec2_close(*result.as_vec2().unwrap(), Vec2::new(1.0, 1.0));
  }

  #[test]
  fn test_lerp_paths_mix_extremes() {
    let src = r#"
path_a = build_path(path {
  move(0, 0)
  line(4, 0)
})
path_b = build_path(path {
  move(0, 10)
  line(4, 10)
})
lerped_a = lerp_paths(path_a, path_b, 0.0)
at_a = lerped_a(0.5)
lerped_b = lerp_paths(path_a, path_b, 1.0)
at_b = lerped_b(0.5)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let at_a = ctx.get_global("at_a").unwrap();
    let at_b = ctx.get_global("at_b").unwrap();
    assert_vec2_close(*at_a.as_vec2().unwrap(), Vec2::new(2.0, 0.0));
    assert_vec2_close(*at_b.as_vec2().unwrap(), Vec2::new(2.0, 10.0));
  }

  #[test]
  fn test_lerp_paths_critical_point_merging() {
    let src = r#"
path_a = build_path(path {
  move(0, 0)
  line(1, 0)
  line(2, 0)
})
path_b = build_path(path {
  move(0, 0)
  line(0.5, 0)
  line(1, 0)
  line(2, 0)
})
lerped = lerp_paths(path_a, path_b, 0.5)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let lerped = ctx.get_global("lerped").unwrap();
    let callable = lerped.as_callable().unwrap();
    let sampler = as_path_sampler(callable).expect("lerped path should be a PathSampler");
    let cps = sampler.critical_t_values();
    // Merged critical points should be sorted and include 0.0 and 1.0
    assert!(cps.len() >= 2);
    assert!((cps[0] - 0.0).abs() < 1e-6);
    assert!((cps[cps.len() - 1] - 1.0).abs() < 1e-6);
    // Should be sorted
    for w in cps.windows(2) {
      assert!(w[0] <= w[1], "Critical points not sorted: {:?}", cps);
    }
    // Should have more points than either path alone (union of both)
    assert!(
      cps.len() > 3,
      "Expected merged critical points from both paths, got {:?}",
      cps
    );
  }

  #[test]
  fn test_critical_points_builtin() {
    let src = r#"
path = build_path(path {
  move(0, 0)
  line(1, 0)
  line(2, 1)
})
cps = critical_points(path)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let cps_val = ctx.get_global("cps").unwrap();
    let seq = cps_val.as_sequence().unwrap();
    let items: Vec<f32> = seq
      .consume(&ctx)
      .map(|r| r.unwrap().as_float().unwrap())
      .collect();
    assert!(
      items.len() >= 3,
      "Expected at least 3 critical points (0, mid, 1), got {:?}",
      items
    );
    assert!((items[0] - 0.0).abs() < 1e-6);
    assert!((items[items.len() - 1] - 1.0).abs() < 1e-6);
    for w in items.windows(2) {
      assert!(w[0] <= w[1], "Critical points not sorted: {:?}", items);
    }
  }

  #[test]
  fn test_critical_points_error_on_generic_callable() {
    let src = r#"
f = |t| { vec2(t, t) }
cps = critical_points(f)
"#;
    let err = parse_and_eval_program(src).unwrap_err();
    let msg = format!("{err}");
    assert!(
      msg.contains("path sampler"),
      "Expected path sampler error, got: {msg}"
    );
  }

  #[test]
  fn test_path_tracer_transform_eval_at() {
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(10.0, 0.0)),
    ];
    let mut tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    // Without transform, eval_at_raw and sample should give same results
    let ctx = crate::EvalCtx::default();
    let p_raw = tracer.eval_at_raw(0.0, &ctx).unwrap();
    assert_vec2_close(p_raw, Vec2::new(0.0, 0.0));

    let p_end_raw = tracer.eval_at_raw(1.0, &ctx).unwrap();
    assert_vec2_close(p_end_raw, Vec2::new(10.0, 0.0));

    // Apply a translation of (5, 3)
    tracer.transform = nalgebra::Matrix3::new(1.0, 0.0, 5.0, 0.0, 1.0, 3.0, 0.0, 0.0, 1.0);

    // eval_at_raw should still return untransformed points
    let p_raw = tracer.eval_at_raw(0.0, &ctx).unwrap();
    assert_vec2_close(p_raw, Vec2::new(0.0, 0.0));

    // eval_at should return transformed points
    let p_transformed = tracer.eval_at(0.0, &ctx).unwrap();
    assert_vec2_close(p_transformed, Vec2::new(5.0, 3.0));

    let p_end_transformed = tracer.eval_at(1.0, &ctx).unwrap();
    assert_vec2_close(p_end_transformed, Vec2::new(15.0, 3.0));

    // Test transform composition: apply a 90-degree rotation on top
    let cos = std::f32::consts::FRAC_PI_2.cos();
    let sin = std::f32::consts::FRAC_PI_2.sin();
    let rot = nalgebra::Matrix3::new(cos, -sin, 0.0, sin, cos, 0.0, 0.0, 0.0, 1.0);
    let new_tracer_box = tracer.with_transform(rot);
    let new_tracer = new_tracer_box
      .as_any()
      .downcast_ref::<PathTracerCallable>()
      .unwrap();

    // Point at t=0 was (0,0) in local space, translated to (5,3), then rotated 90deg
    // Rotation of (5,3) by 90deg = (-3, 5)
    let p = new_tracer.eval_at(0.0, &ctx).unwrap();
    assert_vec2_close(p, Vec2::new(-3.0, 5.0));
  }

  #[test]
  fn test_path_trans_rot_scale_e2e() {
    // Test path_trans with vec2 offset
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0 L 10 10 L 0 10 Z")
moved = path_trans(vec2(5, 3), path)
p0 = moved(0)
p_mid = moved(0.25)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(5.0, 3.0));
    let p_mid = ctx.get_global("p_mid").unwrap();
    assert_vec2_close(*p_mid.as_vec2().unwrap(), Vec2::new(15.0, 3.0));

    // Test path_trans with x, y args
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
moved = path_trans(100, 200, path)
p0 = moved(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(100.0, 200.0));

    // Test path_scale with uniform factor
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
scaled = path_scale(2, path)
p0 = scaled(0)
p1 = scaled(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(0.0, 0.0));
    let p1 = ctx.get_global("p1").unwrap();
    assert_vec2_close(*p1.as_vec2().unwrap(), Vec2::new(20.0, 0.0));

    // Test path_rot — rotate a point (10, 0) by 90 degrees
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
rotated = path_rot(pi / 2, path)
p_end = rotated(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p_end = ctx.get_global("p_end").unwrap();
    assert_vec2_close(*p_end.as_vec2().unwrap(), Vec2::new(0.0, 10.0));

    // Test chaining: translate then rotate
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0")
result = path_rot(pi / 2, path_trans(vec2(5, 0), path))
p0 = result(0)
p1 = result(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(0.0, 5.0));
    let p1 = ctx.get_global("p1").unwrap();
    assert_vec2_close(*p1.as_vec2().unwrap(), Vec2::new(0.0, 15.0));
  }

  #[test]
  fn test_path_reflect_e2e() {
    let src = r#"
path = trace_svg_path("M 2 3 L 8 3")
rx = path_reflect_x(path)
rx_off = path_reflect_x(10, path)
ry = path_reflect_y(path)
ry_off = path | path_reflect_y(10)
diag = path_reflect(vec2(1, 1), path)
diag_off = path_reflect(vec2(1, 0), 4, path)
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
    let g = |name: &str| *ctx.get_global(name).unwrap().as_vec2().unwrap();

    // reflect_x negates y about the x-axis
    assert_vec2_close(g("rx0"), Vec2::new(2.0, -3.0));
    assert_vec2_close(g("rx1"), Vec2::new(8.0, -3.0));
    // reflect_x(10): mirror over y = 10
    assert_vec2_close(g("rx_off0"), Vec2::new(2.0, 17.0));
    // reflect_y negates x about the y-axis
    assert_vec2_close(g("ry0"), Vec2::new(-2.0, 3.0));
    // reflect_y(10) via pipe: mirror over x = 10
    assert_vec2_close(g("ry_off0"), Vec2::new(18.0, 3.0));
    assert_vec2_close(g("ry_off1"), Vec2::new(12.0, 3.0));
    // reflect across the y = x diagonal swaps coords
    assert_vec2_close(g("diag0"), Vec2::new(3.0, 2.0));
    assert_vec2_close(g("diag1"), Vec2::new(3.0, 8.0));
    // reflect across the line running along +x, offset perpendicular by 4 (mirror over y = 4)
    assert_vec2_close(g("diag_off0"), Vec2::new(2.0, 5.0));
  }

  #[test]
  fn test_apply_transforms_and_origin_to_geometry_for_paths() {
    // Test apply_transforms: bake transform into geometry
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0 L 10 10 L 0 10 Z")
moved = path_trans(vec2(100, 200), path)
baked = apply_transforms(moved)
p0 = baked(0)
p1 = baked(0.25)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    // After baking, same world-space position
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(100.0, 200.0));
    let p1 = ctx.get_global("p1").unwrap();
    assert_vec2_close(*p1.as_vec2().unwrap(), Vec2::new(110.0, 200.0));

    // Verify the baked path has identity transform by translating it again
    let src = r#"
path = trace_svg_path("M 0 0 L 10 0 L 10 10 L 0 10 Z")
moved = path_trans(vec2(100, 200), path)
baked = apply_transforms(moved)
moved_again = path_trans(vec2(1, 1), baked)
p0 = moved_again(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(101.0, 201.0));

    // Test origin_to_geometry: centers path geometry
    let src = r#"
path = trace_svg_path("M 10 10 L 20 10 L 20 20 L 10 20 Z")
centered = origin_to_geometry(path)
p0 = centered(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    // Original first point was (10, 10), centroid of endpoints is (15, 15),
    // so centered first point should be (10-15, 10-15) = (-5, -5)
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(-5.0, -5.0));
  }

  /// Verify that `subpath_local_critical_points` maps parent override_critical_points into
  /// per-subpath [0, 1] space correctly.  This is the fix for the oversight where
  /// sample_subpath_points used all segment endpoints as guides instead of the detected sharp
  /// corners stored in override_critical_points.
  #[test]
  fn test_subpath_local_critical_points_with_override() {
    // Two-segment path: subpath 0 is a unit square (length 4), subpath 1 is a unit line
    // (length 1), total length 5.  Place an override critical point at global t=0.2,
    // which is arc-length 1.0 into subpath 0 (the first corner of the square).
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 1.0)),
      DrawCommand::LineTo(Vec2::new(0.0, 1.0)),
      DrawCommand::LineTo(Vec2::new(0.0, 0.0)),
      DrawCommand::MoveTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(11.0, 0.0)),
    ];
    let total_len_expected = 4.0 + 1.0; // square perimeter + unit line

    // Override critical points: 0.0 (start), midpoint of subpath 0 (global t = 2/5 = 0.4),
    // and start of subpath 1 (global t = 4/5 = 0.8).
    let override_cps = vec![0.0, 0.4, 0.8];
    let mut tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    tracer.override_critical_points = Some(override_cps);

    // Sanity: total length is approximately correct.
    assert!((tracer.total_length - total_len_expected).abs() < 0.01);

    // Subpath 0 spans global t [0.0, 0.8] (length 4 / total 5).
    // Override cp at 0.4 is inside subpath 0: local_t = (0.4 - 0.0) / 0.8 = 0.5.
    let local_0 = tracer.subpath_local_critical_points(0);
    assert!(
      local_0.contains(&0.0_f32) && local_0.contains(&1.0_f32),
      "should always include 0.0 and 1.0: {local_0:?}"
    );
    let has_mid = local_0.iter().any(|&t| (t - 0.5).abs() < 1e-4);
    assert!(has_mid, "expected 0.5 in subpath 0 local cps: {local_0:?}");

    // Override cp at 0.8 is exactly the boundary — global_start for subpath 1.
    // Subpath 1 spans global t [0.8, 1.0].  local_t = (0.8 - 0.8) / 0.2 = 0.0.
    let local_1 = tracer.subpath_local_critical_points(1);
    assert!(
      local_1.contains(&0.0_f32) && local_1.contains(&1.0_f32),
      "should include 0.0 and 1.0: {local_1:?}"
    );
  }

  /// Verify that `sample_subpaths_with_limit` on an all-line path:
  ///   1. Returns exactly `limit` points.
  ///   2. Includes the detected critical-point corners (mapped to output points).
  ///   3. When limit >= natural count, returns the natural samples unchanged.
  #[test]
  fn test_sample_subpaths_with_limit_respects_count_and_corners() {
    // Build a closed all-line polygon approximating a circle: 60 line segments.
    use std::f32::consts::TAU;
    let n = 60usize;
    let mut cmds = vec![DrawCommand::MoveTo(Vec2::new(1.0, 0.0))];
    for i in 1..n {
      let angle = TAU * i as f32 / n as f32;
      cmds.push(DrawCommand::LineTo(Vec2::new(angle.cos(), angle.sin())));
    }
    cmds.push(DrawCommand::Close);

    // Mark quarter-circle corners as override critical points (global t = 0, 0.25, 0.5, 0.75).
    let override_cps = vec![0.0, 0.25, 0.5, 0.75];
    let mut tracer = PathTracerCallable::new(true, false, false, cmds, Sym(0));
    tracer.override_critical_points = Some(override_cps.clone());

    // Natural sample count should be 60 (one per segment for closed path).
    let natural = tracer.sample_subpaths(0.1).unwrap();
    let natural_total: usize = natural.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(natural_total, 60);

    // With limit=20 we should get exactly 20 points.
    let limited = tracer.sample_subpaths_with_limit(0.1, Some(20)).unwrap();
    let limited_total: usize = limited.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(
      limited_total, 20,
      "expected exactly 20 points, got {limited_total}"
    );

    // With limit >= natural, result should match natural.
    let no_reduction = tracer.sample_subpaths_with_limit(0.1, Some(100)).unwrap();
    let no_reduction_total: usize = no_reduction.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(no_reduction_total, natural_total);

    // None limit should also match natural.
    let unlimited = tracer.sample_subpaths_with_limit(0.1, None).unwrap();
    let unlimited_total: usize = unlimited.iter().map(|(pts, _)| pts.len()).sum();
    assert_eq!(unlimited_total, natural_total);
  }

  #[test]
  fn test_rect_subpath_corners() {
    // 4-wide, 2-tall rect centered at origin. Perimeter = 12.
    // Corners traced CCW from top-right: (2,1) -> (-2,1) -> (-2,-1) -> (2,-1) -> (2,1).
    // Cumulative arc lengths: 0, 4, 6, 10, 12. As t-fractions: 0, 1/3, 1/2, 5/6, 1.
    let cmds = vec![DrawCommand::Rect {
      center: Vec2::new(0.0, 0.0),
      width: 4.0,
      height: 2.0,
      reversed: false,
    }];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(2.0, 1.0));
    assert_vec2_close(tracer.sample(1.0 / 3.0).unwrap(), Vec2::new(-2.0, 1.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(-2.0, -1.0));
    assert_vec2_close(tracer.sample(5.0 / 6.0).unwrap(), Vec2::new(2.0, -1.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(2.0, 1.0));
  }

  #[test]
  fn test_rect_via_path_block_scalar_size() {
    let src = r#"
path = build_path(path {
  rect(center=v2(0, 0), size=2)
})
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
path = build_path(path {
  rect(center=v2(1, 2), size=v2(4, 6))
})
tr = path(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let tr = ctx.get_global("tr").unwrap();
    assert_vec2_close(*tr.as_vec2().unwrap(), Vec2::new(3.0, 5.0));
  }

  #[test]
  fn test_rect_via_path_block_numeric_form() {
    let src = r#"
path = build_path(path {
  rect(0, 0, 4, 2)
})
tr = path(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let tr = ctx.get_global("tr").unwrap();
    assert_vec2_close(*tr.as_vec2().unwrap(), Vec2::new(2.0, 1.0));
  }

  #[test]
  fn test_path_block_macro_basic() {
    let src = r#"
cmds = path {
  move(0, 0)
  line(1, 0)
  line(1, 1)
}
p = build_path(cmds)
p0 = p(0)
p1 = p(0.25)
p2 = p(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    let p1 = ctx.get_global("p1").unwrap();
    let p2 = ctx.get_global("p2").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(*p1.as_vec2().unwrap(), Vec2::new(0.5, 0.0));
    assert_vec2_close(*p2.as_vec2().unwrap(), Vec2::new(1.0, 1.0));
  }

  #[test]
  fn test_path_block_macro_with_loop_and_flatten() {
    let src = r#"
cmds = path {
  move(0, 0)
  0..10 -> |i| line(i+1, 0)
  close()
}
p = build_path(cmds)
p0 = p(0)
p_end = p(1)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    let p_end = ctx.get_global("p_end").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(0.0, 0.0));
    // close drives last point back to start
    assert_vec2_close(*p_end.as_vec2().unwrap(), Vec2::new(0.0, 0.0));
  }

  #[test]
  fn test_path_block_macro_pipeline() {
    let src = r#"
p = path {
  move(-0.2, -100)
  line(0.2, -100)
  line(0.2, 100)
  line(-0.2, 100)
  close()
} | build_path(center=true)
p0 = p(0)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p0 = ctx.get_global("p0").unwrap();
    assert_vec2_close(*p0.as_vec2().unwrap(), Vec2::new(-0.2, -100.0));
  }

  #[test]
  fn test_path_join_concatenates_subpaths() {
    let src = r#"
a = build_path(path {
  move(0, 0)
  line(1, 0)
})
b = build_path(path {
  move(10, 0)
  line(11, 0)
})
joined = path_join(a, b)
p_a = joined(0.25)
p_b = joined(0.75)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p_a = ctx.get_global("p_a").unwrap();
    let p_b = ctx.get_global("p_b").unwrap();
    assert_vec2_close(*p_a.as_vec2().unwrap(), Vec2::new(0.5, 0.0));
    assert_vec2_close(*p_b.as_vec2().unwrap(), Vec2::new(10.5, 0.0));
  }

  #[test]
  fn test_path_join_bakes_non_identity_transforms() {
    // Same two segments as above, but each side carries a translate transform that must be
    // baked into the joined geometry rather than silently dropped.
    let src = r#"
a = build_path(path {
  move(0, 0)
  line(1, 0)
}) | path_trans(v2(5, 0))
b = build_path(path {
  move(0, 0)
  line(1, 0)
}) | path_trans(v2(20, 0))
joined = path_join(a, b)
p_a = joined(0.25)
p_b = joined(0.75)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let p_a = ctx.get_global("p_a").unwrap();
    let p_b = ctx.get_global("p_b").unwrap();
    // a runs (5,0)→(6,0), b runs (20,0)→(21,0). Joined parametric t hits midpoint of each.
    assert_vec2_close(*p_a.as_vec2().unwrap(), Vec2::new(5.5, 0.0));
    assert_vec2_close(*p_b.as_vec2().unwrap(), Vec2::new(20.5, 0.0));
  }

  #[test]
  fn test_path_block_return_disallowed() {
    let src = r#"
cmds = path {
  return move(0, 0)
}
"#;
    let err = parse_and_eval_program(src).unwrap_err();
    let msg = format!("{err}");
    assert!(
      msg.contains("`return` is not allowed"),
      "expected return-not-allowed error, got: {msg}"
    );
  }

  #[test]
  fn test_path_block_empty_is_empty_path() {
    let src = r#"
cmds = path {}
p = build_path(cmds)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let _ = ctx.get_global("p").unwrap();
  }

  #[test]
  fn test_build_segment_dicts_lines_and_global_t() {
    // Two open subpaths with lengths 1 and 3 → global total = 4.
    // Subpath 0 has two segments (t_start/t_end ∈ [0, 0.5, 1] locally).
    let cmds = vec![
      DrawCommand::MoveTo(Vec2::new(0.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(0.5, 0.0)),
      DrawCommand::LineTo(Vec2::new(1.0, 0.0)),
      DrawCommand::MoveTo(Vec2::new(10.0, 0.0)),
      DrawCommand::LineTo(Vec2::new(13.0, 0.0)),
    ];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let dicts = build_segment_dicts(&tracer).unwrap();
    let unwrap_map = |v: &Value| match v {
      Value::Map(m) => m.clone(),
      _ => panic!("expected Map, got {v:?}"),
    };
    let f = |m: &FxHashMap<String, Value>, k: &str| m.get(k).and_then(|v| v.as_float()).unwrap();
    let i = |m: &FxHashMap<String, Value>, k: &str| m.get(k).and_then(|v| v.as_int()).unwrap();
    let s = |m: &FxHashMap<String, Value>, k: &str| {
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
    // Verifies that path_segments is wired up as a builtin, returns a Sequence,
    // and the consumed dicts have the renamed `type` key.
    let src = r#"
p = build_path(path {
  move(0, 0)
  line(1, 0)
  line(1, 1)
})
segs = path_segments(p)
types = segs -> |s, _i| s.type
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let types = ctx.get_global("types").unwrap();
    let seq = types.as_sequence().unwrap();
    let collected: Vec<String> = seq
      .consume(&ctx)
      .map(|r| r.unwrap().as_str().unwrap().to_owned())
      .collect();
    assert_eq!(collected, vec!["line".to_owned(), "line".to_owned()]);
  }

  #[test]
  fn test_path_frame_unit_circle_inward_normal() {
    // Build a unit circle (CCW). Sample at t=0.25 — pos should be ~the leftmost point of the
    // circle (build_path's circle starts at the right and goes CCW), tangent should point
    // downward (-y), and inward normal should point toward origin (+x).
    let src = r#"
p = build_path(path { circle(v2(0), 1) })
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

    // pos should be on the unit circle at the t=0.25 point of a CCW circle starting at (1,0).
    assert!(
      (pos.norm() - 1.0).abs() < 1e-2,
      "pos not on unit circle: {pos:?}"
    );
    // tangent should be perpendicular to pos (since for circles d/dt of pos is tangential).
    assert!(
      tangent.dot(&pos).abs() < 5e-2,
      "tangent not perp to radial: tangent={tangent:?}, pos={pos:?}"
    );
    // inward normal points from pos toward origin → normal should be ~ -pos.
    let inward = -pos.normalize();
    assert!(
      (normal - inward).norm() < 5e-2,
      "normal not inward: normal={normal:?}, expected~={inward:?}"
    );
  }

  #[test]
  fn test_path_frame_inward_normal_errors_on_bare_lambda() {
    // A raw lambda has no path topology, so `inward_normal=true` (the default) should hard-error
    // rather than silently produce whatever the left-perpendicular happens to be.
    let src = r#"
p = |t| v2(cos(t * tau), sin(t * tau))
f = path_frame(0.25, p)
"#;
    let err = parse_and_eval_program(src).unwrap_err();
    let msg = err.to_string();
    assert!(
      msg.contains("inward normal") || msg.contains("topology"),
      "expected inward-normal/topology error, got: {msg}"
    );
  }

  #[test]
  fn test_path_frame_inward_normal_false_works_on_bare_lambda() {
    // With inward_normal=false the same lambda should sample successfully.
    let src = r#"
p = |t| v2(cos(t * tau), sin(t * tau))
f = path_frame(0.25, p, inward_normal=false)
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let frame = ctx.get_global("f").unwrap();
    assert!(matches!(frame, Value::Map(_)));
  }

  #[test]
  fn test_discretize_path_replaces_curves_with_lines() {
    // A circle path normally has two arc segments. After discretize_path, every segment must
    // be a `line`. Closedness must be preserved.
    let src = r#"
p = build_path(path { circle(v2(0), 5) })
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
  }

  #[test]
  fn test_build_segment_dicts_arc_fields() {
    let cmds = vec![DrawCommand::Circle {
      center: Vec2::new(0.0, 0.0),
      radius: 5.0,
      reversed: false,
    }];
    let tracer = PathTracerCallable::new(false, false, false, cmds, Sym(0));
    let dicts = build_segment_dicts(&tracer).unwrap();
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
