use std::{hash::Hasher, rc::Rc};

use super::{Path, SubpathTopology};
use crate::{
  builtins::trace_path::normalize_guides, path_building::eval_cardinal_spline, Callable,
  ErrorStack, EvalCtx, Value, Vec2, EMPTY_KWARGS,
};

pub(crate) trait AbstractPath {
  fn name(&self) -> &'static str;
  fn eval_raw(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack>;
  /// Local `t`, sorted, within `[0, 1]`; empty when nothing is known.
  fn critical_t_values(&self) -> Vec<f32>;
  fn topology(&self) -> Option<SubpathTopology> {
    None
  }
  fn content_hash(&self, _h: &mut dyn Hasher) -> bool {
    false
  }
  /// User callable driving this path, for the optimizer's purity checks.
  fn closure(&self) -> Option<&Rc<Callable>> {
    None
  }
  fn children(&self) -> Vec<&Rc<Path>> {
    Vec::new()
  }
}

pub(crate) struct ClosurePath {
  pub f: Rc<Callable>,
  pub closed: Option<bool>,
}

impl AbstractPath for ClosurePath {
  fn name(&self) -> &'static str {
    "path(fn)"
  }

  fn eval_raw(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    let val = ctx
      .invoke_callable(&self.f, &[Value::Float(t)], EMPTY_KWARGS)
      .map_err(|e| e.wrap("Error invoking path callable"))?;
    val
      .as_vec2()
      .copied()
      .ok_or_else(|| ErrorStack::new(format!("path callable returned a non-vec2 value: {val:?}")))
  }

  fn critical_t_values(&self) -> Vec<f32> {
    Vec::new()
  }

  fn topology(&self) -> Option<SubpathTopology> {
    self.closed.map(|closed| SubpathTopology { closed })
  }

  fn closure(&self) -> Option<&Rc<Callable>> {
    Some(&self.f)
  }
}

pub(crate) struct LerpPath {
  pub a: Rc<Path>,
  pub b: Rc<Path>,
  pub mix: f32,
  merged_critical: Vec<f32>,
}

impl LerpPath {
  /// Critical points merge the inputs'; with neither known, `sample_count + 1` uniform values.
  pub(crate) fn new(a: Rc<Path>, b: Rc<Path>, mix: f32, sample_count: usize) -> Self {
    let (mut ca, mut cb) = (a.critical_t_values(), b.critical_t_values());
    let merged_critical = if ca.is_empty() && cb.is_empty() {
      let count = sample_count + 1;
      (0..count)
        .map(|i| i as f32 / (count - 1).max(1) as f32)
        .collect()
    } else {
      ca.append(&mut cb);
      normalize_guides(&ca)
    };
    Self {
      a,
      b,
      mix,
      merged_critical,
    }
  }
}

impl AbstractPath for LerpPath {
  fn name(&self) -> &'static str {
    "lerp_paths"
  }

  fn eval_raw(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    if self.mix <= 0.0 {
      return self.a.eval_at(t, ctx);
    }
    if self.mix >= 1.0 {
      return self.b.eval_at(t, ctx);
    }
    let a = self.a.eval_at(t, ctx)?;
    let b = self.b.eval_at(t, ctx)?;
    Ok(a + (b - a) * self.mix)
  }

  fn critical_t_values(&self) -> Vec<f32> {
    self.merged_critical.clone()
  }

  fn children(&self) -> Vec<&Rc<Path>> {
    vec![&self.a, &self.b]
  }
}

pub(crate) struct CatmullRom2D {
  pub points: Vec<Vec2>,
  pub tension: f32,
  pub closed: bool,
}

impl AbstractPath for CatmullRom2D {
  fn name(&self) -> &'static str {
    "catmull_rom"
  }

  fn eval_raw(&self, t: f32, _ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    Ok(eval_cardinal_spline(
      &self.points,
      t,
      self.tension,
      self.closed,
    ))
  }

  fn critical_t_values(&self) -> Vec<f32> {
    vec![0.0, 1.0]
  }

  fn topology(&self) -> Option<SubpathTopology> {
    Some(SubpathTopology {
      closed: self.closed,
    })
  }

  fn content_hash(&self, h: &mut dyn Hasher) -> bool {
    for p in &self.points {
      h.write_u32(p.x.to_bits());
      h.write_u32(p.y.to_bits());
    }
    h.write_u32(self.tension.to_bits());
    h.write_u8(self.closed as u8);
    true
  }
}

/// `[start_t, start_t + span]` of `inner`, remapped to `[0, 1]`.
pub(crate) struct Trimmed {
  pub inner: Rc<Path>,
  pub start_t: f32,
  pub span: f32,
  critical: Vec<f32>,
}

impl Trimmed {
  pub(crate) fn new(inner: Rc<Path>, start_t: f32, end_t: f32) -> Self {
    let span = end_t - start_t;
    let critical = inner
      .critical_t_values()
      .into_iter()
      .filter(|&t| t >= start_t - 1e-6 && t <= end_t + 1e-6)
      .map(|t| ((t - start_t) / span).clamp(0.0, 1.0))
      .collect();
    Self {
      inner,
      start_t,
      span,
      critical,
    }
  }
}

impl AbstractPath for Trimmed {
  fn name(&self) -> &'static str {
    "trim_path"
  }

  fn eval_raw(&self, t: f32, ctx: &EvalCtx) -> Result<Vec2, ErrorStack> {
    let g = (self.start_t + t.clamp(0.0, 1.0) * self.span).clamp(0.0, 1.0);
    self.inner.eval_at(g, ctx)
  }

  fn critical_t_values(&self) -> Vec<f32> {
    self.critical.clone()
  }

  fn children(&self) -> Vec<&Rc<Path>> {
    vec![&self.inner]
  }
}
