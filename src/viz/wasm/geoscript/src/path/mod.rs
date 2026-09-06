//! First-class 2D paths. A `Path` is one concrete subpath, a group of paths, or a lazy sampler,
//! with lazy affine/reverse modifiers that are baked eagerly on concrete trees (see
//! `docs/path-system-impl-spec.md`).

pub(crate) mod builder;
pub(crate) mod centroid;
pub(crate) mod lazy;
pub(crate) mod segment;
pub(crate) mod subpath;
#[cfg(test)]
mod tests;

use std::{cell::OnceCell, cmp::Ordering, fmt, hash::Hasher, rc::Rc};

use nalgebra::Matrix3;

pub(crate) use builder::{DrawCommand, PathBuilder};
pub(crate) use lazy::AbstractPath;
pub(crate) use segment::*;
pub(crate) use subpath::{LastCtrl, Subpath};

use crate::{builtins::trace_path::build_topology_samples, ErrorStack, EvalCtx, Value, Vec2};

const LAZY_LENGTH_SAMPLES: usize = 512;
const LAZY_CLOSED_EPSILON: f32 = 1e-4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FillRule {
  NonZero,
  EvenOdd,
  Positive,
  Negative,
}

impl FillRule {
  pub(crate) fn accepts(self, winding: i32) -> bool {
    match self {
      FillRule::NonZero => winding != 0,
      FillRule::EvenOdd => winding & 1 == 1,
      FillRule::Positive => winding > 0,
      FillRule::Negative => winding < 0,
    }
  }

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

  pub(crate) fn name(self) -> &'static str {
    match self {
      FillRule::NonZero => "nonzero",
      FillRule::EvenOdd => "evenodd",
      FillRule::Positive => "positive",
      FillRule::Negative => "negative",
    }
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
pub(crate) struct SubpathTopology {
  pub closed: bool,
}

pub struct Path {
  pub(crate) kind: PathKind,
  /// Lazy modifiers; identity / `false` on concrete trees, which bake them eagerly.
  pub(crate) transform: Matrix3<f32>,
  pub(crate) reverse: bool,
  pub(crate) fill_rule: Option<FillRule>,
  length: OnceCell<f32>,
}

#[derive(Clone)]
pub(crate) enum PathKind {
  Subpath(Subpath),
  Group(Group),
  Abstract(Rc<dyn AbstractPath>),
}

/// The only source of multiplicity. Child spans are proportional to length in the group's
/// local `t`; lazy children contribute their sampled estimate.
#[derive(Clone)]
pub(crate) struct Group {
  pub(crate) children: Vec<Rc<Path>>,
  pub(crate) cumulative_lengths: Vec<f32>,
  pub(crate) total_length: f32,
}

impl Group {
  fn from_lengths(children: Vec<Rc<Path>>, lengths: impl Iterator<Item = f32>) -> Self {
    let mut cumulative_lengths = Vec::with_capacity(children.len());
    let mut total_length = 0.;
    for len in lengths {
      total_length += len;
      cumulative_lengths.push(total_length);
    }
    Self {
      children,
      cumulative_lengths,
      total_length,
    }
  }

  pub(crate) fn concrete(children: Vec<Rc<Path>>) -> Self {
    let lengths: Vec<f32> = children
      .iter()
      .map(|c| match &c.kind {
        PathKind::Subpath(sp) => sp.total_length(),
        _ => unreachable!("concrete group child must be a leaf"),
      })
      .collect();
    Self::from_lengths(children, lengths.into_iter())
  }

  pub(crate) fn new(children: Vec<Rc<Path>>, ctx: &EvalCtx) -> Result<Self, ErrorStack> {
    let lengths = children
      .iter()
      .map(|c| c.length(ctx))
      .collect::<Result<Vec<_>, _>>()?;
    Ok(Self::from_lengths(children, lengths.into_iter()))
  }

  fn child_start(&self, ix: usize) -> f32 {
    if ix == 0 {
      0.
    } else {
      self.cumulative_lengths[ix - 1]
    }
  }

  fn locate(&self, target: f32) -> usize {
    let ix = match self
      .cumulative_lengths
      .binary_search_by(|len| len.partial_cmp(&target).unwrap_or(Ordering::Less))
    {
      Ok(ix) => ix,
      Err(ix) => ix,
    };
    ix.min(self.children.len() - 1)
  }

  fn eval_raw(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    if self.children.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Err(ErrorStack::new("empty path"));
    }
    let target = t * self.total_length;
    let ix = self.locate(target);
    let child = &self.children[ix];
    let local_len = target - self.child_start(ix);
    match &child.kind {
      PathKind::Subpath(sp) => Ok(sp.sample_by_length(local_len.clamp(0., sp.total_length()))),
      _ => {
        let len = self.cumulative_lengths[ix] - self.child_start(ix);
        let local_t = if len <= LENGTH_EPSILON {
          0.
        } else {
          (local_len / len).clamp(0., 1.)
        };
        child.eval_at(local_t, ctx)
      }
    }
  }

  fn spans(&self) -> Vec<(f32, f32)> {
    (0..self.children.len())
      .map(|ix| {
        (
          (self.child_start(ix) / self.total_length).clamp(0., 1.),
          (self.cumulative_lengths[ix] / self.total_length).clamp(0., 1.),
        )
      })
      .collect()
  }
}

fn max_singular_value(m: &Matrix3<f32>) -> f32 {
  let (a, b, c, d) = (m.m11, m.m12, m.m21, m.m22);
  let (sum_sq, det) = (a * a + b * b + c * c + d * d, a * d - b * c);
  (0.5 * (sum_sq + (sum_sq * sum_sq - 4. * det * det).max(0.).sqrt())).sqrt()
}

fn uniform_scale(m: &Matrix3<f32>) -> Option<f32> {
  is_uniform_transform(m).then(|| (m.m11 * m.m11 + m.m21 * m.m21).sqrt())
}

impl Path {
  fn with_kind(kind: PathKind) -> Path {
    Path {
      kind,
      transform: Matrix3::identity(),
      reverse: false,
      fill_rule: None,
      length: OnceCell::new(),
    }
  }

  pub(crate) fn leaf(sp: Subpath) -> Path {
    Path::with_kind(PathKind::Subpath(sp))
  }

  pub(crate) fn lazy(inner: Rc<dyn AbstractPath>) -> Path {
    Path::with_kind(PathKind::Abstract(inner))
  }

  pub(crate) fn empty() -> Path {
    Path::concrete_group(Vec::new(), None)
  }

  /// Normalized group over leaf children.
  pub(crate) fn concrete_group(children: Vec<Rc<Path>>, fill_rule: Option<FillRule>) -> Path {
    debug_assert!(children
      .iter()
      .all(|c| matches!(c.kind, PathKind::Subpath(_)) && c.is_normalized()));
    let mut p = Path::with_kind(PathKind::Group(Group::concrete(children)));
    p.fill_rule = fill_rule;
    p
  }

  fn is_normalized(&self) -> bool {
    !self.reverse && self.transform == Matrix3::identity()
  }

  pub(crate) fn is_concrete(&self) -> bool {
    self.first_lazy_name().is_none()
  }

  pub(crate) fn first_lazy_name(&self) -> Option<&'static str> {
    match &self.kind {
      PathKind::Subpath(_) => None,
      PathKind::Abstract(a) => Some(a.name()),
      PathKind::Group(g) => g.children.iter().find_map(|c| c.first_lazy_name()),
    }
  }

  /// Leaves of a normalized concrete tree.
  pub(crate) fn leaves(&self) -> Vec<&Subpath> {
    match &self.kind {
      PathKind::Subpath(sp) => vec![sp],
      PathKind::Group(g) => g
        .children
        .iter()
        .map(|c| match &c.kind {
          PathKind::Subpath(sp) => sp,
          _ => panic!("leaves() on a lazy path"),
        })
        .collect(),
      PathKind::Abstract(_) => panic!("leaves() on a lazy path"),
    }
  }

  fn leaf_children(&self, out: &mut Vec<Rc<Path>>, into: fn(&Subpath) -> Subpath) {
    match &self.kind {
      PathKind::Subpath(sp) => out.push(Rc::new(Path::leaf(into(sp)))),
      PathKind::Group(g) => {
        for c in &g.children {
          c.leaf_children(out, into);
        }
      }
      PathKind::Abstract(_) => unreachable!(),
    }
  }

  /// Groups `items` (§4.1): concrete inputs merge into one normalized level, otherwise the
  /// items are kept as lazy children.
  pub(crate) fn group_items(
    items: Vec<Rc<Path>>,
    fill_rule: Option<FillRule>,
    ctx: &EvalCtx,
  ) -> Result<Path, ErrorStack> {
    let fill_rule = match fill_rule {
      Some(r) => Some(r),
      None => {
        let mut rule = None;
        for item in &items {
          match (rule, item.fill_rule) {
            (Some(a), Some(b)) if a != b => {
              return Err(ErrorStack::new(format!(
                "`path`: conflicting fill rules ({} vs {}); pass fill_rule= explicitly",
                FillRule::name(a),
                FillRule::name(b)
              )));
            }
            (None, Some(b)) => rule = Some(b),
            _ => {}
          }
        }
        rule
      }
    };
    if items.iter().all(|p| p.is_concrete()) {
      let mut children = Vec::with_capacity(items.len());
      for item in &items {
        match &item.kind {
          PathKind::Subpath(_) => children.push(Rc::clone(item)),
          PathKind::Group(g) => children.extend(g.children.iter().cloned()),
          PathKind::Abstract(_) => unreachable!(),
        }
      }
      return Ok(Path::concrete_group(children, fill_rule));
    }
    let mut p = Path::with_kind(PathKind::Group(Group::new(items, ctx)?));
    p.fill_rule = fill_rule;
    Ok(p)
  }

  /// Children of a group (a leaf yields itself); a lazy group's modifiers are pushed down.
  pub(crate) fn subpaths(self: &Rc<Path>) -> Vec<Rc<Path>> {
    let PathKind::Group(g) = &self.kind else {
      return vec![Rc::clone(self)];
    };
    let mut out: Vec<Rc<Path>> = g
      .children
      .iter()
      .map(|c| {
        if self.is_normalized() {
          Rc::clone(c)
        } else {
          let mut p = c.transformed(&self.transform);
          if self.reverse {
            p = p.reversed();
          }
          Rc::new(p)
        }
      })
      .collect();
    if self.reverse {
      out.reverse();
    }
    out
  }

  fn lazy_copy(&self) -> Path {
    Path {
      kind: self.kind.clone(),
      transform: self.transform,
      reverse: self.reverse,
      fill_rule: self.fill_rule,
      length: OnceCell::new(),
    }
  }

  pub(crate) fn transformed(&self, m: &Matrix3<f32>) -> Path {
    if *m == Matrix3::identity() {
      return self.lazy_copy();
    }
    let mut out = match &self.kind {
      PathKind::Subpath(sp) => Path::leaf(sp.transformed(m)),
      PathKind::Group(_) if self.is_concrete() => {
        let leaves = self
          .leaves()
          .into_iter()
          .map(|sp| Rc::new(Path::leaf(sp.transformed(m))))
          .collect();
        Path::concrete_group(leaves, None)
      }
      _ => {
        let mut p = self.lazy_copy();
        p.transform = m * self.transform;
        p
      }
    };
    out.fill_rule = self.fill_rule;
    out
  }

  pub(crate) fn reversed(&self) -> Path {
    let mut out = match &self.kind {
      PathKind::Subpath(sp) => Path::leaf(sp.reversed()),
      PathKind::Group(_) if self.is_concrete() => {
        let leaves = self
          .leaves()
          .into_iter()
          .rev()
          .map(|sp| Rc::new(Path::leaf(sp.reversed())))
          .collect();
        Path::concrete_group(leaves, None)
      }
      _ => {
        let mut p = self.lazy_copy();
        p.reverse = !self.reverse;
        p
      }
    };
    out.fill_rule = self.fill_rule;
    out
  }

  pub(crate) fn with_fill_rule(&self, fill_rule: Option<FillRule>) -> Path {
    let mut out = self.lazy_copy();
    out.fill_rule = fill_rule;
    out
  }

  pub(crate) fn translated(&self, offset: Vec2) -> Path {
    self.transformed(&Matrix3::new_translation(&offset))
  }

  pub(crate) fn origin_to_geometry(&self, ctx: &EvalCtx) -> Result<Path, ErrorStack> {
    Ok(match self.centroid(ctx)? {
      Some(c) => self.translated(-c),
      None => self.lazy_copy(),
    })
  }

  pub(crate) fn eval_at(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    let t = t.clamp(0., 1.);
    let t = if self.reverse { 1.0 - t } else { t };
    let p = match &self.kind {
      PathKind::Subpath(sp) => {
        if sp.is_degenerate() {
          return Err(ErrorStack::new("empty path"));
        }
        sp.sample_t(t)
      }
      PathKind::Group(g) => g.eval_raw(t, ctx)?,
      PathKind::Abstract(a) => a.eval_raw(t, ctx)?,
    };
    Ok(apply_transform_to_point(&self.transform, p))
  }

  fn sampled_length(&self, ctx: &EvalCtx) -> Result<f32, ErrorStack> {
    let n = LAZY_LENGTH_SAMPLES;
    let mut prev = self.eval_at(0., ctx)?;
    let mut total = 0.;
    for i in 1..=n {
      let p = self.eval_at(i as f32 / n as f32, ctx)?;
      total += (p - prev).norm();
      prev = p;
    }
    Ok(total)
  }

  pub(crate) fn length(&self, ctx: &EvalCtx) -> Result<f32, ErrorStack> {
    if let Some(len) = self.length.get() {
      return Ok(*len);
    }
    let len = match &self.kind {
      PathKind::Subpath(sp) => sp.total_length(),
      PathKind::Group(g) => match uniform_scale(&self.transform) {
        Some(s) => g.total_length * s,
        None => self.sampled_length(ctx)?,
      },
      PathKind::Abstract(_) => self.sampled_length(ctx)?,
    };
    Ok(*self.length.get_or_init(|| len))
  }

  pub(crate) fn critical_t_values(&self) -> Vec<f32> {
    let mut out = match &self.kind {
      PathKind::Subpath(sp) => sp.critical_t_values(),
      PathKind::Abstract(a) => a.critical_t_values(),
      PathKind::Group(g) => {
        let mut out = Vec::new();
        if g.total_length > LENGTH_EPSILON {
          for (ix, child) in g.children.iter().enumerate() {
            let (start, end) = (g.child_start(ix), g.cumulative_lengths[ix]);
            for t in child.critical_t_values() {
              let global = ((start + t * (end - start)) / g.total_length).clamp(0., 1.);
              if out.last() != Some(&global) {
                out.push(global);
              }
            }
          }
        }
        out
      }
    };
    if self.reverse {
      for t in &mut out {
        *t = 1.0 - *t;
      }
      out.reverse();
    }
    out
  }

  pub(crate) fn subpath_topology(&self) -> Option<Vec<SubpathTopology>> {
    let mut out = match &self.kind {
      PathKind::Subpath(sp) => vec![SubpathTopology { closed: sp.closed }],
      PathKind::Abstract(a) => vec![a.topology()?],
      PathKind::Group(g) => {
        let mut out = Vec::with_capacity(g.children.len());
        for c in &g.children {
          out.extend(c.subpath_topology()?);
        }
        out
      }
    };
    if self.reverse {
      out.reverse();
    }
    Some(out)
  }

  pub(crate) fn subpath_t_spans(&self) -> Option<Vec<(f32, f32)>> {
    let mut spans = match &self.kind {
      PathKind::Group(g) => {
        if g.children.is_empty() || g.total_length <= LENGTH_EPSILON {
          return None;
        }
        let mut spans = Vec::new();
        for (ix, c) in g.children.iter().enumerate() {
          let (start, end) = (g.child_start(ix), g.cumulative_lengths[ix]);
          let len = end - start;
          for (a, b) in c.subpath_t_spans()? {
            spans.push((
              ((start + a * len) / g.total_length).clamp(0., 1.),
              ((start + b * len) / g.total_length).clamp(0., 1.),
            ));
          }
        }
        spans
      }
      _ => vec![(0., 1.)],
    };
    if self.reverse {
      for s in &mut spans {
        *s = (1.0 - s.1, 1.0 - s.0);
      }
      spans.reverse();
    }
    Some(spans)
  }

  /// Black-box fallback: `lazy_count` uniform samples as one polyline, closedness from the
  /// topology when known and from `p(0) ≈ p(1)` otherwise.
  fn sample_lazy(&self, lazy_count: usize, ctx: &EvalCtx) -> Result<(Vec<Vec2>, bool), ErrorStack> {
    let closed = match &self.kind {
      PathKind::Abstract(a) => a.topology().map(|t| t.closed),
      _ => None,
    };
    let closed = match closed {
      Some(c) => c,
      None => (self.eval_at(0., ctx)? - self.eval_at(1., ctx)?).norm() <= LAZY_CLOSED_EPSILON,
    };
    let ts = build_topology_samples(lazy_count, None, None, !closed);
    let mut points = Vec::with_capacity(ts.len());
    for t in ts {
      points.push(self.eval_at(t, ctx)?);
    }
    Ok((points, closed))
  }

  /// One `(points, closed)` polyline per subpath in world space. Concrete leaves flatten
  /// adaptively (chords turn at most `angle_tolerance`, sag at most `max_sagitta`); lazy leaves
  /// fall back to `lazy_count` uniform samples.
  pub(crate) fn sample_subpaths_flat(
    &self,
    angle_tolerance: f32,
    max_sagitta: f32,
    lazy_count: usize,
    ctx: &EvalCtx,
  ) -> Result<Vec<(Vec<Vec2>, bool)>, ErrorStack> {
    let mut out = match &self.kind {
      PathKind::Subpath(sp) => {
        vec![(
          sp.sample_points(angle_tolerance, max_sagitta, !sp.closed),
          sp.closed,
        )]
      }
      PathKind::Abstract(_) => return Ok(vec![self.sample_lazy(lazy_count, ctx)?]),
      PathKind::Group(g) => {
        let sagitta = max_sagitta / max_singular_value(&self.transform).max(1e-12);
        let mut out = Vec::with_capacity(g.children.len());
        for c in &g.children {
          out.extend(c.sample_subpaths_flat(angle_tolerance, sagitta, lazy_count, ctx)?);
        }
        out
      }
    };
    if self.reverse {
      out.reverse();
      for (points, _) in &mut out {
        points.reverse();
      }
    }
    if self.transform != Matrix3::identity() {
      for (points, _) in &mut out {
        for p in points {
          *p = apply_transform_to_point(&self.transform, *p);
        }
      }
    }
    Ok(out)
  }

  pub(crate) fn sample_subpaths(
    &self,
    angle_tolerance: f32,
    lazy_count: usize,
    ctx: &EvalCtx,
  ) -> Result<Vec<(Vec<Vec2>, bool)>, ErrorStack> {
    self.sample_subpaths_flat(angle_tolerance, f32::INFINITY, lazy_count, ctx)
  }

  /// `sample_subpaths` capped at `total_limit` points across all subpaths, redistributing by
  /// curvature + arc-length mass while keeping explicit anchors as boundaries.
  pub(crate) fn sample_subpaths_with_limit(
    &self,
    angle_tolerance: f32,
    total_limit: Option<usize>,
    ctx: &EvalCtx,
  ) -> Result<Vec<(Vec<Vec2>, bool)>, ErrorStack> {
    use crate::mesh_ops::adaptive_sampler::{
      adaptive_sample, distribute_samples_by_mass, recommended_n_dense, DEFAULT_MIN_SEGMENT_LENGTH,
    };

    if !self.is_concrete() {
      return self.sample_subpaths(angle_tolerance, total_limit.unwrap_or(64), ctx);
    }
    let Some(limit) = total_limit else {
      return self.sample_subpaths(angle_tolerance, 64, ctx);
    };
    let natural = self.sample_subpaths(angle_tolerance, 64, ctx)?;
    let natural_total: usize = natural.iter().map(|(pts, _)| pts.len()).sum();
    if limit >= natural_total {
      return Ok(natural);
    }

    let leaves = self.leaves();
    let n_subpaths = leaves.len();
    let free_budget = limit.saturating_sub(n_subpaths);
    let n_dense = recommended_n_dense(limit, n_subpaths);
    let allocations = {
      let samplers: Vec<_> = leaves.iter().map(|sp| |t: f32| sp.sample_t(t)).collect();
      distribute_samples_by_mass::<Vec2, _>(free_budget, &samplers, n_dense)
    };

    let mut result = Vec::with_capacity(n_subpaths);
    for (sp, &extra) in leaves.iter().zip(&allocations) {
      let boundaries = sp.resample_boundaries();
      let ts = adaptive_sample::<Vec2, _>(
        1 + extra,
        &boundaries,
        |t| sp.sample_t(t),
        DEFAULT_MIN_SEGMENT_LENGTH,
      );
      let points = ts.iter().map(|&t| sp.sample_t(t)).collect();
      result.push((points, sp.closed));
    }
    Ok(result)
  }

  pub(crate) fn to_lyon_path(&self) -> Option<lyon_tessellation::path::Path> {
    use lyon_tessellation::{
      geom::{Angle, Arc, Point, Vector},
      path::Path as LyonPath,
    };
    if !self.is_concrete() {
      return None;
    }
    let pt = |p: Vec2| Point::new(p.x, p.y);
    let mut builder = LyonPath::builder();
    for sp in self.leaves() {
      if sp.segments.is_empty() {
        continue;
      }
      builder.begin(pt(sp.segments[0].start_point()));
      for seg in &sp.segments {
        match seg {
          PathSegment::Line { end, .. } => {
            builder.line_to(pt(*end));
          }
          PathSegment::Quadratic { ctrl, end, .. } => {
            builder.quadratic_bezier_to(pt(*ctrl), pt(*end));
          }
          PathSegment::Cubic {
            ctrl1, ctrl2, end, ..
          } => {
            builder.cubic_bezier_to(pt(*ctrl1), pt(*ctrl2), pt(*end));
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
            let arc = Arc {
              center: Point::new(center.x, center.y),
              radii: Vector::new(*rx, *ry),
              start_angle: Angle::radians(*theta_start),
              sweep_angle: Angle::radians(*theta_delta),
              x_rotation: Angle::radians(sin_phi.atan2(*cos_phi)),
            };
            arc.for_each_cubic_bezier(&mut |c| {
              builder.cubic_bezier_to(c.ctrl1, c.ctrl2, c.to);
            });
          }
        }
      }
      builder.end(sp.closed);
    }
    Some(builder.build())
  }

  pub(crate) fn aabb(&self, ctx: &EvalCtx) -> Result<Option<(Vec2, Vec2)>, ErrorStack> {
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    let mut any = false;
    if self.is_concrete() {
      for (smin, smax) in self.leaves().into_iter().filter_map(Subpath::aabb) {
        min = min.inf(&smin);
        max = max.sup(&smax);
        any = true;
      }
    } else {
      for (points, _) in self.sample_subpaths(5f32.to_radians(), LAZY_LENGTH_SAMPLES, ctx)? {
        for p in points {
          min = min.inf(&p);
          max = max.sup(&p);
          any = true;
        }
      }
    }
    Ok(any.then_some((min, max)))
  }

  /// Signed-area centroid of the filled region when any subpath is closed (holes subtract),
  /// otherwise the arc-length centroid of every segment.
  pub(crate) fn centroid(&self, ctx: &EvalCtx) -> Result<Option<Vec2>, ErrorStack> {
    if !self.is_concrete() {
      let polylines = self.sample_subpaths(5f32.to_radians(), LAZY_LENGTH_SAMPLES, ctx)?;
      return Path::from_polylines(polylines, None).centroid(ctx);
    }
    let leaves = self.leaves();
    let closed = leaves.iter().filter(|sp| sp.closed && !sp.is_degenerate());
    let region = centroid::region_centroid(closed.map(|sp| sp.segments.as_slice()));
    Ok(
      region
        .or_else(|| centroid::arc_length_centroid(leaves.iter().flat_map(|sp| sp.segments.iter()))),
    )
  }

  /// Index of the leaf covering global `t` on a concrete tree.
  pub(crate) fn leaf_index_at(&self, t: f32) -> Option<usize> {
    match &self.kind {
      PathKind::Subpath(_) => Some(0),
      PathKind::Group(g) => {
        if g.children.is_empty() || g.total_length <= 0. {
          return None;
        }
        let target = t.clamp(0., 1.) * g.total_length;
        Some(
          g.cumulative_lengths
            .iter()
            .position(|&len| target <= len)
            .unwrap_or(g.children.len() - 1),
        )
      }
      PathKind::Abstract(_) => None,
    }
  }

  /// Exact slice of `[start_t, end_t]` (sampling `t`) for concrete trees; a lazy `Trimmed`
  /// wrapper otherwise. Partially covered subpaths become open.
  pub(crate) fn trimmed(self: &Rc<Path>, start_t: f32, end_t: f32) -> Path {
    if !self.is_concrete() {
      let mut p = Path::lazy(Rc::new(lazy::Trimmed::new(Rc::clone(self), start_t, end_t)));
      p.fill_rule = self.fill_rule;
      return p;
    }
    let leaves = self.leaves();
    let total: f32 = leaves.iter().map(|sp| sp.total_length()).sum();
    let (start_len, end_len) = (start_t * total, end_t * total);
    let mut out = Vec::new();
    let mut offset = 0.;
    for sp in leaves {
      let (sp_start, sp_end) = (offset, offset + sp.total_length());
      offset = sp_end;
      let (lo, hi) = (start_len.max(sp_start), end_len.min(sp_end));
      if hi - lo <= LENGTH_EPSILON {
        continue;
      }
      let (local_lo, local_hi) = (lo - sp_start, hi - sp_start);
      let fully = local_lo <= LENGTH_EPSILON && local_hi >= sp.total_length() - LENGTH_EPSILON;
      let mut segments = Vec::new();
      let mut anchors = Vec::new();
      let mut seg_offset = 0.;
      for (seg, &anchor) in sp.segments.iter().zip(&sp.anchors) {
        let (seg_start, seg_end) = (seg_offset, seg_offset + seg.length());
        seg_offset = seg_end;
        let (s, e) = (local_lo.max(seg_start), local_hi.min(seg_end));
        if e - s <= LENGTH_EPSILON {
          continue;
        }
        let u = seg.param_for_length(s - seg_start);
        let v = seg.param_for_length(e - seg_start);
        if let Some(sliced) = seg.slice(u, v) {
          segments.push(sliced);
          anchors.push(anchor || s > seg_start + LENGTH_EPSILON);
        }
      }
      if segments.is_empty() {
        continue;
      }
      let start = segments[0].start_point();
      out.push(Rc::new(Path::leaf(Subpath::new(
        start,
        segments,
        fully && sp.closed,
        anchors,
        sp.explicit_anchors,
        None,
      ))));
    }
    Path::concrete_group(out, self.fill_rule)
  }

  /// Every user callable reachable through lazy leaves.
  pub(crate) fn closures(&self) -> Vec<Rc<crate::Callable>> {
    fn walk(p: &Path, out: &mut Vec<Rc<crate::Callable>>) {
      match &p.kind {
        PathKind::Subpath(_) => {}
        PathKind::Group(g) => g.children.iter().for_each(|c| walk(c, out)),
        PathKind::Abstract(a) => {
          out.extend(a.closure().cloned());
          a.children().iter().for_each(|c| walk(c, out));
        }
      }
    }
    let mut out = Vec::new();
    walk(self, &mut out);
    out
  }

  /// `true` when the tree hashed structurally; `false` means fall back to identity.
  pub(crate) fn content_hash(&self, h: &mut dyn Hasher) -> bool {
    for v in self.transform.iter() {
      h.write_u32(v.to_bits());
    }
    h.write_u8(self.reverse as u8);
    h.write_u8(
      self
        .fill_rule
        .map(|r| r.to_clipper2_u32() as u8 + 1)
        .unwrap_or(0),
    );
    match &self.kind {
      PathKind::Subpath(sp) => {
        h.write_u8(0);
        sp.content_hash(h);
        true
      }
      PathKind::Group(g) => {
        h.write_u8(1);
        h.write_usize(g.children.len());
        g.children.iter().all(|c| c.content_hash(h))
      }
      PathKind::Abstract(a) => {
        h.write_u8(2);
        a.content_hash(h)
      }
    }
  }
}

impl fmt::Debug for Path {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let mods = format!(
      "{}{}",
      if self.transform != Matrix3::identity() {
        " | transform"
      } else {
        ""
      },
      if self.reverse { " | reverse" } else { "" }
    );
    match &self.kind {
      PathKind::Abstract(a) => write!(f, "<path: {}{mods}>", a.name()),
      PathKind::Group(_) | PathKind::Subpath(_) if self.is_concrete() => {
        let leaves = self.leaves();
        let len: f32 = leaves.iter().map(|sp| sp.total_length()).sum();
        write!(f, "<path: {} subpaths, len {len:.4}>", leaves.len())
      }
      PathKind::Group(g) => write!(f, "<path: {} children (lazy){mods}>", g.children.len()),
      PathKind::Subpath(_) => unreachable!(),
    }
  }
}
