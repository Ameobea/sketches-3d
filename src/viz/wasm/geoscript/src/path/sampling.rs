//! Tolerance-driven materialization of lazy paths. Structural breakpoints and crease flags
//! are separate: a polygon joint must bound refinement even when it isn't a crease.

use super::{Path, PathKind, LAZY_CLOSED_EPSILON};
use crate::{ErrorStack, EvalCtx, Vec2};

pub(crate) struct SampledSubpath {
  pub points: Vec<Vec2>,
  pub anchors: Vec<bool>,
  pub closed: bool,
}

// Termination safeguards, not a replacement for the requested tolerance. At f32 resolution
// further subdivision cannot improve the geometry. Excessive work fails rather than silently
// returning a coarse boundary to a boolean engine.
const MAX_EVALUATIONS: usize = 1_000_000;
const MAX_DEPTH: usize = 24;

struct Sampler<'a> {
  path: &'a Path,
  ctx: &'a EvalCtx,
  angle: f64,
  sagitta: f64,
  evaluations: usize,
  points: Vec<Vec2>,
  anchors: Vec<bool>,
}

impl Sampler<'_> {
  fn eval(&mut self, t: f32) -> Result<Vec2, ErrorStack> {
    if self.evaluations >= MAX_EVALUATIONS {
      return Err(ErrorStack::new(
        "Lazy path materialization exceeded its evaluation limit; increase curve_angle_degrees or \
         simplify the input path",
      ));
    }
    self.evaluations += 1;
    let p = self.path.eval_at(t, self.ctx)?;
    if !p.iter().all(|v| v.is_finite()) {
      return Err(ErrorStack::new(format!(
        "Lazy path materialization encountered a non-finite point at t={t}"
      )));
    }
    Ok(p)
  }

  fn push(&mut self, p: Vec2, anchor: bool) {
    self.points.push(p);
    self.anchors.push(anchor);
  }

  fn refine(
    &mut self,
    a: f32,
    b: f32,
    p: Vec2,
    q: Vec2,
    end_anchor: bool,
    depth: usize,
  ) -> Result<(), ErrorStack> {
    let mid = a + (b - a) * 0.5;
    if mid <= a || mid >= b {
      self.push(q, end_anchor);
      return Ok(());
    }
    // Quarter probes catch inflections and excursions a midpoint-only test would miss.
    let ts = [a + (b - a) * 0.25, mid, a + (b - a) * 0.75];
    let probes = [self.eval(ts[0])?, self.eval(ts[1])?, self.eval(ts[2])?];
    let samples = [p, probes[0], probes[1], probes[2], q];
    let scale = samples
      .iter()
      .fold(0f64, |s, v| s.max(v.x.abs() as f64).max(v.y.abs() as f64));
    let noise = (scale * 8. * f32::EPSILON as f64).max(f32::MIN_POSITIVE as f64);
    let mut turn = 0.;
    let mut previous: Option<nalgebra::Vector2<f64>> = None;
    for pair in samples.windows(2) {
      let edge = pair[1].cast::<f64>() - pair[0].cast::<f64>();
      if edge.norm() <= noise {
        continue;
      }
      if let Some(prev) = previous {
        turn += (prev.x * edge.y - prev.y * edge.x)
          .abs()
          .atan2(prev.dot(&edge));
      }
      previous = Some(edge);
    }
    let start = p.cast::<f64>();
    let chord = q.cast::<f64>() - start;
    let len_sq = chord.norm_squared();
    let deviation = probes.iter().fold(0f64, |d, probe| {
      let v = probe.cast::<f64>() - start;
      let along = if len_sq > 0. {
        (v.dot(&chord) / len_sq).clamp(0., 1.)
      } else {
        0.
      };
      d.max((v - chord * along).norm())
    });
    if deviation <= noise
      || (turn <= self.angle * 0.5 && deviation <= self.sagitta.max(noise))
      || samples
        .iter()
        .all(|v| (v.cast::<f64>() - start).norm() <= noise)
    {
      self.push(q, end_anchor);
    } else if depth == MAX_DEPTH {
      // A non-guide corner or f32 noise can persist at arbitrarily small parameter spans.
      // Retain all probes at the precision floor; never promote them to crease anchors.
      for probe in probes {
        self.push(probe, false);
      }
      self.push(q, end_anchor);
    } else {
      self.refine(a, mid, p, probes[1], false, depth + 1)?;
      self.refine(mid, b, probes[1], q, end_anchor, depth + 1)?;
    }
    Ok(())
  }
}

pub(super) fn sample_lazy(
  path: &Path,
  angle: f32,
  sagitta: f32,
  seed_count: usize,
  ctx: &EvalCtx,
) -> Result<SampledSubpath, ErrorStack> {
  if !angle.is_finite() || angle <= 0. || sagitta.is_nan() || sagitta <= 0. {
    return Err(ErrorStack::new(
      "Invalid lazy path sampling tolerance; expected positive values",
    ));
  }
  if seed_count > MAX_EVALUATIONS / 4 {
    return Err(ErrorStack::new(
      "Lazy path sample_count exceeds the materialization limit",
    ));
  }
  let mut sampler = Sampler {
    path,
    ctx,
    angle: angle as f64,
    sagitta: sagitta as f64,
    evaluations: 0,
    points: Vec::new(),
    anchors: Vec::new(),
  };
  let first = sampler.eval(0.)?;
  let last = sampler.eval(1.)?;
  let closed = match &path.kind {
    PathKind::Abstract(a) => a.topology().map(|t| t.closed),
    _ => None,
  }
  .unwrap_or_else(|| (first - last).norm() <= LAZY_CLOSED_EPSILON);
  let critical = path.critical_t_values();
  let linear = path.is_piecewise_linear();
  let mut seeds: Vec<(f32, bool)> = path
    .sampling_t_values()
    .into_iter()
    .map(|t| (t, false))
    .chain(critical.into_iter().map(|t| (t, true)))
    .collect();
  let intervals = if linear {
    // Lerps/trims of polylines are exactly linear between the merged input joints. Uniform
    // samples add no information here and amplify f32 evaluation noise into tiny zigzags.
    1
  } else if closed {
    seed_count.max(2)
  } else {
    seed_count.max(2) - 1
  };
  seeds.extend((0..=intervals).map(|i| (i as f32 / intervals as f32, false)));
  seeds.retain(|(t, _)| t.is_finite() && (0.0..=1.0).contains(t));
  seeds.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
  // Exact deduplication: even very close, explicitly distinct guide values must be sampled.
  seeds.dedup_by(|a, b| {
    if a.0 != b.0 {
      return false;
    }
    b.1 |= a.1;
    true
  });
  sampler.push(first, seeds[0].1);
  let mut p = first;
  for pair in seeds.windows(2) {
    let q = if pair[1].0 == 1. {
      last
    } else {
      sampler.eval(pair[1].0)?
    };
    if linear {
      sampler.push(q, pair[1].1);
    } else {
      sampler.refine(pair[0].0, pair[1].0, p, q, pair[1].1, 0)?;
    }
    p = q;
  }
  if closed && (first - last).norm() <= LAZY_CLOSED_EPSILON {
    sampler.points.pop();
    let end_anchor = sampler.anchors.pop().unwrap();
    sampler.anchors[0] |= end_anchor;
  }
  Ok(SampledSubpath {
    points: sampler.points,
    anchors: sampler.anchors,
    closed,
  })
}
