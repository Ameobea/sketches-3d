use std::cmp::Ordering;
use std::f32::consts::PI;
use std::rc::Rc;

use fxhash::FxHashMap;
use svgtypes::PathParser;

use crate::{
  ast::{ClosureBody, Expr, FunctionCall, FunctionCallTarget},
  builtins::{
    fn_defs::{fn_sigs, get_builtin_fn_sig_entry_ix},
    FUNCTION_ALIASES,
  },
  get_args, AppendOnlyBuffer, ArgRef, ArgType, Callable, CapturedScope, Closure, DynamicCallable,
  ErrorStack, EvalCtx, GetArgsOutput, Scope, Sym, Value, Vec2, EMPTY_ARGS, EMPTY_KWARGS,
};

const CURVE_TABLE_SAMPLES: usize = 32;
const LENGTH_EPSILON: f32 = 1e-5;

fn extend_bounds(min: &mut Vec2, max: &mut Vec2, p: Vec2) {
  min.x = min.x.min(p.x);
  min.y = min.y.min(p.y);
  max.x = max.x.max(p.x);
  max.y = max.y.max(p.y);
}

#[derive(Clone)]
struct ArcLengthTable {
  cumulative: Vec<f32>,
  total: f32,
}

impl ArcLengthTable {
  fn new(samples: usize, mut sample_fn: impl FnMut(f32) -> Vec2) -> (Self, Vec2, Vec2) {
    let samples = samples.max(1);
    let mut cumulative = Vec::with_capacity(samples + 1);
    let mut total = 0.0;

    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    let mut prev = sample_fn(0.0);
    extend_bounds(&mut min, &mut max, prev);
    cumulative.push(0.0);

    for i in 1..=samples {
      let t = i as f32 / samples as f32;
      let point = sample_fn(t);
      extend_bounds(&mut min, &mut max, point);
      total += (point - prev).norm();
      cumulative.push(total);
      prev = point;
    }

    (Self { cumulative, total }, min, max)
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

#[derive(Clone)]
enum PathSegment {
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

impl PathSegment {
  fn translate(&mut self, offset: Vec2) {
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

  fn end(&self) -> Vec2 {
    match self {
      PathSegment::Line { end, .. } => *end,
      PathSegment::Quadratic { end, .. } => *end,
      PathSegment::Cubic { end, .. } => *end,
      PathSegment::Arc { end, .. } => *end,
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

pub struct PathTracerCallable {
  interned_t_kwarg: Sym,
  segments: Vec<PathSegment>,
  cumulative_lengths: Vec<f32>,
  total_length: f32,
}

impl PathTracerCallable {
  pub fn new(
    closed: bool,
    center: bool,
    draw_cmds: Vec<DrawCommand>,
    interned_t_kwarg: Sym,
  ) -> Self {
    let mut segments = Vec::new();
    let mut current: Option<Vec2> = None;
    let mut first_point: Option<Vec2> = None;
    let mut last_cubic_ctrl: Option<Vec2> = None;
    let mut last_quad_ctrl: Option<Vec2> = None;

    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    let get_start = |current: &Option<Vec2>, first_point: &Option<Vec2>| -> Vec2 {
      current
        .or(*first_point)
        .unwrap_or_else(|| Vec2::new(0.0, 0.0))
    };

    for cmd in draw_cmds {
      match cmd {
        DrawCommand::MoveTo(pos) => {
          extend_bounds(&mut min, &mut max, pos);
          current = Some(pos);
          if first_point.is_none() {
            first_point = Some(pos);
          }
          last_cubic_ctrl = None;
          last_quad_ctrl = None;
        }
        DrawCommand::LineTo(pos) => {
          let start = get_start(&current, &first_point);
          extend_bounds(&mut min, &mut max, start);
          extend_bounds(&mut min, &mut max, pos);
          let length = (pos - start).norm();
          if length > LENGTH_EPSILON {
            segments.push(PathSegment::Line {
              start,
              end: pos,
              length,
            });
          }
          current = Some(pos);
          last_cubic_ctrl = None;
          last_quad_ctrl = None;
        }
        DrawCommand::QuadraticBezier { ctrl, to } => {
          let start = get_start(&current, &first_point);
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            quadratic_bezier(start, ctrl, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            segments.push(PathSegment::Quadratic {
              start,
              ctrl,
              end: to,
              table,
            });
          }
          current = Some(to);
          last_quad_ctrl = Some(ctrl);
          last_cubic_ctrl = None;
        }
        DrawCommand::SmoothQuadraticBezier { to } => {
          let start = get_start(&current, &first_point);
          let ctrl = match last_quad_ctrl {
            Some(last_ctrl) => start + (start - last_ctrl),
            None => start,
          };
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            quadratic_bezier(start, ctrl, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            segments.push(PathSegment::Quadratic {
              start,
              ctrl,
              end: to,
              table,
            });
          }
          current = Some(to);
          last_quad_ctrl = Some(ctrl);
          last_cubic_ctrl = None;
        }
        DrawCommand::CubicBezier { ctrl1, ctrl2, to } => {
          let start = get_start(&current, &first_point);
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            cubic_bezier(start, ctrl1, ctrl2, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            segments.push(PathSegment::Cubic {
              start,
              ctrl1,
              ctrl2,
              end: to,
              table,
            });
          }
          current = Some(to);
          last_cubic_ctrl = Some(ctrl2);
          last_quad_ctrl = None;
        }
        DrawCommand::SmoothCubicBezier { ctrl2, to } => {
          let start = get_start(&current, &first_point);
          let ctrl1 = match last_cubic_ctrl {
            Some(last_ctrl) => start + (start - last_ctrl),
            None => start,
          };
          let (table, tmin, tmax) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
            cubic_bezier(start, ctrl1, ctrl2, to, t)
          });
          extend_bounds(&mut min, &mut max, tmin);
          extend_bounds(&mut min, &mut max, tmax);
          if table.total() > LENGTH_EPSILON {
            segments.push(PathSegment::Cubic {
              start,
              ctrl1,
              ctrl2,
              end: to,
              table,
            });
          }
          current = Some(to);
          last_cubic_ctrl = Some(ctrl2);
          last_quad_ctrl = None;
        }
        DrawCommand::Arc {
          rx,
          ry,
          x_axis_rotation,
          large_arc,
          sweep,
          to,
        } => {
          let start = get_start(&current, &first_point);
          if let Some((segment, tmin, tmax)) =
            build_arc_segment(start, to, rx, ry, x_axis_rotation, large_arc, sweep)
          {
            extend_bounds(&mut min, &mut max, tmin);
            extend_bounds(&mut min, &mut max, tmax);
            if segment.length() > LENGTH_EPSILON {
              segments.push(segment);
            }
          }
          current = Some(to);
          last_cubic_ctrl = None;
          last_quad_ctrl = None;
        }
        DrawCommand::Close => {
          if let (Some(cur), Some(first)) = (current, first_point) {
            extend_bounds(&mut min, &mut max, cur);
            extend_bounds(&mut min, &mut max, first);
            let length = (first - cur).norm();
            if length > LENGTH_EPSILON {
              segments.push(PathSegment::Line {
                start: cur,
                end: first,
                length,
              });
            }
            current = Some(first);
          }
          last_cubic_ctrl = None;
          last_quad_ctrl = None;
        }
      }
    }

    if closed {
      if let Some(cur) = current {
        let start = first_point.unwrap_or(cur);
        extend_bounds(&mut min, &mut max, cur);
        extend_bounds(&mut min, &mut max, start);
        let length = (start - cur).norm();
        if length > LENGTH_EPSILON {
          segments.push(PathSegment::Line {
            start: cur,
            end: start,
            length,
          });
        }
      }
    }

    if center && min.x <= max.x {
      let center_pt = (min + max) * 0.5;
      let offset = -center_pt;
      for segment in &mut segments {
        segment.translate(offset);
      }
    }

    let mut cumulative_lengths = Vec::with_capacity(segments.len());
    let mut total_length = 0.0;
    for segment in &segments {
      total_length += segment.length();
      cumulative_lengths.push(total_length);
    }

    Self {
      interned_t_kwarg,
      segments,
      cumulative_lengths,
      total_length,
    }
  }

  fn sample(&self, t: f32) -> Result<Vec2, ErrorStack> {
    if self.segments.is_empty() || self.total_length <= LENGTH_EPSILON {
      return Err(ErrorStack::new(
        "trace_path path has no drawable segments to sample",
      ));
    }

    let target = t * self.total_length;
    let mut idx = match self
      .cumulative_lengths
      .binary_search_by(|len| len.partial_cmp(&target).unwrap_or(Ordering::Less))
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
      return Ok(seg.end());
    }
    let local_len = (target - seg_start_len).clamp(0.0, seg_len);
    Ok(seg.sample_by_length(local_len))
  }
}

impl DynamicCallable for PathTracerCallable {
  fn invoke(
    &self,
    args: &[crate::Value],
    kwargs: &FxHashMap<Sym, Value>,
    _ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let t = if !kwargs.is_empty() {
      if kwargs.len() != 1 || !kwargs.contains_key(&self.interned_t_kwarg) {
        return Err(ErrorStack::new(
          "Unexpected keyword arguments; expected only `t`",
        ));
      }
      if !args.is_empty() {
        return Err(ErrorStack::new(
          "Expected only keyword argument `t` and no positional args",
        ));
      }
      kwargs.get(&self.interned_t_kwarg).unwrap()
    } else {
      if args.len() != 1 {
        return Err(ErrorStack::new("Expected argument `t`"));
      }
      &args[0]
    };
    let Some(t) = t.as_float() else {
      return Err(ErrorStack::new(format!(
        "Expected 't' to be a number, found {t:?}"
      )));
    };
    let t = t.clamp(0., 1.);

    let pos = self.sample(t)?;
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

#[derive(Clone)]
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
  Close,
}

struct DrawCtx {
  pub cmds: AppendOnlyBuffer<DrawCommand>,
}

impl Default for DrawCtx {
  fn default() -> Self {
    Self {
      cmds: AppendOnlyBuffer::default(),
    }
  }
}

impl DrawCtx {
  fn into_inner(&self) -> Vec<DrawCommand> {
    // there might be references to this floating around, and who cares about a clone here anyway
    self.cmds.borrow().to_vec()
  }
}

fn inject_draw_commands(ctx: &EvalCtx, scope: &Scope, draw_ctx: &Rc<DrawCtx>) {
  fn draw_command_kind_for_name(name: &str) -> Option<DrawCommandKind> {
    match name {
      "move" => Some(DrawCommandKind::Move),
      "line" => Some(DrawCommandKind::Line),
      "quadratic_bezier" => Some(DrawCommandKind::Quadratic),
      "smooth_quadratic_bezier" => Some(DrawCommandKind::SmoothQuadratic),
      "cubic_bezier" => Some(DrawCommandKind::Cubic),
      "smooth_cubic_bezier" => Some(DrawCommandKind::SmoothCubic),
      "arc" => Some(DrawCommandKind::Arc),
      "close" => Some(DrawCommandKind::Close),
      _ => None,
    }
  }

  fn insert_cmd(
    ctx: &EvalCtx,
    scope: &Scope,
    draw_ctx: &Rc<DrawCtx>,
    name: &'static str,
    kind: DrawCommandKind,
  ) {
    scope.insert(
      ctx.interned_symbols.intern(name),
      Value::Callable(Rc::new(Callable::Dynamic {
        name: format!("trace_path.{name}"),
        inner: Box::new(DrawCommandCallable {
          fn_name: name,
          kind,
          draw_ctx: Rc::clone(draw_ctx),
        }),
      })),
    );
  }

  let canonical = [
    "move",
    "line",
    "quadratic_bezier",
    "smooth_quadratic_bezier",
    "cubic_bezier",
    "smooth_cubic_bezier",
    "arc",
    "close",
  ];
  for name in canonical {
    if let Some(kind) = draw_command_kind_for_name(name) {
      insert_cmd(ctx, scope, draw_ctx, name, kind);
    }
  }

  // Trace-path-specific alias; global aliasing maps "bezier" to 3d.
  insert_cmd(ctx, scope, draw_ctx, "bezier", DrawCommandKind::Cubic);

  for (alias, target) in FUNCTION_ALIASES.entries() {
    if let Some(kind) = draw_command_kind_for_name(target) {
      insert_cmd(ctx, scope, draw_ctx, alias, kind);
    }
  }
}

#[derive(Clone, Copy)]
enum DrawCommandKind {
  Move,
  Line,
  Quadratic,
  SmoothQuadratic,
  Cubic,
  SmoothCubic,
  Arc,
  Close,
}

struct DrawCommandCallable {
  fn_name: &'static str,
  kind: DrawCommandKind,
  draw_ctx: Rc<DrawCtx>,
}

impl DrawCommandCallable {
  fn fn_name(&self) -> &'static str {
    self.fn_name
  }
}

impl DynamicCallable for DrawCommandCallable {
  fn invoke(
    &self,
    args: &[Value],
    kwargs: &FxHashMap<Sym, Value>,
    ctx: &EvalCtx,
  ) -> Result<Value, ErrorStack> {
    let fn_name = self.fn_name();
    let resolved_name = match fn_name {
      "quad_bezier" => "quadratic_bezier",
      "smooth_quad_bezier" => "smooth_quadratic_bezier",
      "smooth_bezier" => "smooth_cubic_bezier",
      "bezier" => "cubic_bezier",
      _ => fn_name,
    };
    let fn_def = fn_sigs()
      .get(resolved_name)
      .ok_or_else(|| ErrorStack::new(format!("Unknown draw command `{fn_name}`")))?;
    let (def_ix, arg_refs) = match get_args(ctx, fn_name, fn_def.signatures, args, kwargs)? {
      GetArgsOutput::Valid { def_ix, arg_refs } => (def_ix, arg_refs),
      GetArgsOutput::PartiallyApplied => {
        return Err(ErrorStack::new(
          "Draw commands do not support partial application",
        ))
      }
    };

    match self.kind {
      DrawCommandKind::Move => {
        let pos = match def_ix {
          0 => {
            let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            Vec2::new(x, y)
          }
          1 => *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
          _ => unreachable!(),
        };
        self.draw_ctx.cmds.push(DrawCommand::MoveTo(pos));
      }
      DrawCommandKind::Line => {
        let pos = match def_ix {
          0 => {
            let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            Vec2::new(x, y)
          }
          1 => *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
          _ => unreachable!(),
        };
        self.draw_ctx.cmds.push(DrawCommand::LineTo(pos));
      }
      DrawCommandKind::Quadratic => {
        let (ctrl, to) = match def_ix {
          0 => (
            *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[1].resolve(args, kwargs).as_vec2().unwrap(),
          ),
          1 => {
            let cx = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let cy = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            let x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            (Vec2::new(cx, cy), Vec2::new(x, y))
          }
          _ => unreachable!(),
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::QuadraticBezier { ctrl, to });
      }
      DrawCommandKind::SmoothQuadratic => {
        let to = match def_ix {
          0 => *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
          1 => {
            let x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            Vec2::new(x, y)
          }
          _ => unreachable!(),
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::SmoothQuadraticBezier { to });
      }
      DrawCommandKind::Cubic => {
        let (ctrl1, ctrl2, to) = match def_ix {
          0 => (
            *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[1].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[2].resolve(args, kwargs).as_vec2().unwrap(),
          ),
          1 => {
            let c1x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let c1y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            let c2x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
            let c2y = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            let x = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
            (Vec2::new(c1x, c1y), Vec2::new(c2x, c2y), Vec2::new(x, y))
          }
          _ => {
            return Err(ErrorStack::new(format!(
              "`{fn_name}` cannot be used with Vec3 inputs inside `trace_path`; use `bezier3d` \
               outside of `trace_path` or `cubic_bezier` with Vec2 values"
            )))
          }
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::CubicBezier { ctrl1, ctrl2, to });
      }
      DrawCommandKind::SmoothCubic => {
        let (ctrl2, to) = match def_ix {
          0 => (
            *arg_refs[0].resolve(args, kwargs).as_vec2().unwrap(),
            *arg_refs[1].resolve(args, kwargs).as_vec2().unwrap(),
          ),
          1 => {
            let c2x = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
            let c2y = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
            let x = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            (Vec2::new(c2x, c2y), Vec2::new(x, y))
          }
          _ => unreachable!(),
        };
        self
          .draw_ctx
          .cmds
          .push(DrawCommand::SmoothCubicBezier { ctrl2, to });
      }
      DrawCommandKind::Arc => {
        let rx = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
        let ry = arg_refs[1].resolve(args, kwargs).as_float().unwrap();
        let x_axis_rotation = arg_refs[2].resolve(args, kwargs).as_float().unwrap();
        let (large_arc, sweep, to) = match def_ix {
          0 => {
            let large_arc = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
            let sweep = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();
            let x = arg_refs[5].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[6].resolve(args, kwargs).as_float().unwrap();
            (large_arc, sweep, Vec2::new(x, y))
          }
          1 => {
            let large_arc = arg_refs[3].resolve(args, kwargs).as_bool().unwrap();
            let sweep = arg_refs[4].resolve(args, kwargs).as_bool().unwrap();
            let to = *arg_refs[5].resolve(args, kwargs).as_vec2().unwrap();
            (large_arc, sweep, to)
          }
          2 => {
            let x = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
            let y = arg_refs[4].resolve(args, kwargs).as_float().unwrap();
            (false, true, Vec2::new(x, y))
          }
          3 => {
            let to = *arg_refs[3].resolve(args, kwargs).as_vec2().unwrap();
            (false, true, to)
          }
          _ => unreachable!(),
        };
        self.draw_ctx.cmds.push(DrawCommand::Arc {
          rx,
          ry,
          x_axis_rotation,
          large_arc,
          sweep,
          to,
        });
      }
      DrawCommandKind::Close => {
        self.draw_ctx.cmds.push(DrawCommand::Close);
      }
    }

    Ok(Value::Nil)
  }

  fn get_return_type_hint(&self) -> Option<ArgType> {
    Some(ArgType::Nil)
  }

  fn is_side_effectful(&self) -> bool {
    true
  }

  fn is_rng_dependent(&self) -> bool {
    false
  }
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
) -> Option<(PathSegment, Vec2, Vec2)> {
  let mut rx = rx.abs();
  let mut ry = ry.abs();
  if rx <= LENGTH_EPSILON || ry <= LENGTH_EPSILON {
    let length = (end - start).norm();
    let mut min = start;
    let mut max = start;
    extend_bounds(&mut min, &mut max, end);
    return Some((PathSegment::Line { start, end, length }, min, max));
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
    let mut min = start;
    let mut max = start;
    extend_bounds(&mut min, &mut max, end);
    return Some((PathSegment::Line { start, end, length }, min, max));
  }

  let numerator = rx_sq * ry_sq - rx_sq * y1p_sq - ry_sq * x1p_sq;
  let coef = (numerator / denom).max(0.0).sqrt();
  let sign = if large_arc == sweep { -1.0 } else { 1.0 };
  let coef = sign * coef;

  let cxp = coef * (rx * y1p / ry);
  let cyp = coef * (-ry * x1p / rx);
  let cx = cos_phi * cxp - sin_phi * cyp + (start.x + end.x) / 2.0;
  let cy = sin_phi * cxp + cos_phi * cyp + (start.y + end.y) / 2.0;
  let center = Vec2::new(cx, cy);

  let v1 = Vec2::new((x1p - cxp) / rx, (y1p - cyp) / ry);
  let v2 = Vec2::new((-x1p - cxp) / rx, (-y1p - cyp) / ry);
  let theta_start = v1.y.atan2(v1.x);
  let mut theta_delta = (v1.x * v2.y - v1.y * v2.x).atan2(v1.x * v2.x + v1.y * v2.y);

  if !sweep && theta_delta > 0.0 {
    theta_delta -= 2.0 * PI;
  } else if sweep && theta_delta < 0.0 {
    theta_delta += 2.0 * PI;
  }

  let (table, min, max) = ArcLengthTable::new(CURVE_TABLE_SAMPLES, |t| {
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

  Some((
    PathSegment::Arc {
      end,
      center,
      rx,
      ry,
      cos_phi,
      sin_phi,
      theta_start,
      theta_delta,
      table,
    },
    min,
    max,
  ))
}

pub(crate) const TRACE_PATH_DRAW_COMMAND_NAMES: [&str; 8] = [
  "move",
  "line",
  "quadratic_bezier",
  "smooth_quadratic_bezier",
  "cubic_bezier",
  "smooth_cubic_bezier",
  "arc",
  "close",
];

fn eval_trace_path_cb(ctx: &EvalCtx, cb: &Callable) -> Result<Vec<DrawCommand>, ErrorStack> {
  let Callable::Closure(closure) = cb else {
    return Err(ErrorStack::new(
      "You must pass a closure directly to `trace_path`'s callback argument.  The closure's scope \
       is specially modified to make the path drawing commands available.",
    ));
  };

  let captured_scope = match &closure.captured_scope {
    CapturedScope::Strong(scope) => Rc::clone(&scope),
    CapturedScope::Weak(weak) => {
      log::error!("I'm pretty sure this isn't possible except in recursive call cases...");
      weak.upgrade().ok_or_else(|| {
        ErrorStack::new("Internal error: captured scope has been dropped unexpectedly")
      })?
    }
  };

  let wrapped_scope = Scope::wrap(captured_scope);

  let draw_ctx = Rc::new(DrawCtx::default());
  inject_draw_commands(ctx, &wrapped_scope, &draw_ctx);

  let mut closure: Closure = closure.clone();

  // Const folding will also work against us by inserting builtin callable literals mapping to the
  // placeholder draw command stubs that just error out.
  //
  // We have to traverse the closure body and replace them with the actual draw command callables.
  let mut body: ClosureBody = (*closure.body).clone();

  let mut draw_cmd_name_by_entry_ix = FxHashMap::default();
  for name in TRACE_PATH_DRAW_COMMAND_NAMES {
    let entry_ix = get_builtin_fn_sig_entry_ix(name).unwrap();
    draw_cmd_name_by_entry_ix.insert(entry_ix, name);
  }
  let mut traverse = |expr: &mut Expr| {
    fn traverse_inner(
      ctx: &EvalCtx,
      draw_cmd_name_by_entry_ix: &FxHashMap<usize, &str>,
      expr: &mut Expr,
    ) {
      match expr {
        Expr::Call(FunctionCall { target, .. }) => match target {
          FunctionCallTarget::Literal(callable) => match &**callable {
            Callable::Builtin { fn_entry_ix, .. } => {
              dbg!(fn_sigs().entries[*fn_entry_ix].0);
              if let Some(name) = draw_cmd_name_by_entry_ix.get(fn_entry_ix) {
                *target = FunctionCallTarget::Name(ctx.interned_symbols.intern(name));
              }
            }
            _ => (),
          },
          _ => (),
        },
        // users can define helper functions inside the closure that also use draw commands
        Expr::Closure {
          body: inner_body, ..
        } => {
          let mut new_helper_body: ClosureBody = (**inner_body).clone();
          let mut traverse_helper = |expr: &mut Expr| {
            traverse_inner(ctx, draw_cmd_name_by_entry_ix, expr);
          };
          new_helper_body.traverse_exprs_mut(&mut traverse_helper);
          *inner_body = Rc::new(new_helper_body);
        }
        _ => (),
      }
    }

    traverse_inner(ctx, &draw_cmd_name_by_entry_ix, expr);
  };
  body.traverse_exprs_mut(&mut traverse);
  closure.body = Rc::new(body);

  closure.captured_scope = CapturedScope::Strong(Rc::new(wrapped_scope));
  ctx
    .invoke_closure(&closure, EMPTY_ARGS, EMPTY_KWARGS)
    .map_err(|err| err.wrap("Error while executing user-provided path tracing callback"))?;

  Ok(draw_ctx.into_inner())
}

pub(crate) fn draw_command_stub_impl(
  name: &'static str,
  _def_ix: usize,
  _arg_refs: &[ArgRef],
  _args: &[Value],
  _kwargs: &FxHashMap<Sym, Value>,
  _ctx: &EvalCtx,
) -> Result<Value, ErrorStack> {
  Err(ErrorStack::new(format!(
    "`{name}` can only be called within the callback passed to `trace_path`",
  )))
}

pub fn trace_path_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  match def_ix {
    0 => {
      let cb = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
      let closed = arg_refs[1].resolve(args, kwargs).as_bool().unwrap();
      let center = arg_refs[2].resolve(args, kwargs).as_bool().unwrap();

      let draw_cmds = eval_trace_path_cb(ctx, cb)
        .map_err(|err| err.wrap("Error while evaluating callback provided to `trace_path`"))?;

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let path_tracer = PathTracerCallable::new(closed, center, draw_cmds, interned_t_kwarg);
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "trace_path".to_string(),
        inner: Box::new(path_tracer),
      })))
    }
    _ => unimplemented!(),
  }
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

      let draw_cmds = parse_svg_path_to_draw_commands(svg_path_str)
        .map_err(|err| err.wrap("Error while parsing SVG path string"))?;

      let interned_t_kwarg = ctx.interned_symbols.intern("t");
      let path_tracer = PathTracerCallable::new(false, center, draw_cmds, interned_t_kwarg);
      Ok(Value::Callable(Rc::new(Callable::Dynamic {
        name: "trace_svg_path".to_string(),
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
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.25).unwrap(), Vec2::new(1.0, 0.0));
    assert_vec2_close(tracer.sample(0.75).unwrap(), Vec2::new(1.0, 2.0));
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
    let tracer = PathTracerCallable::new(false, true, cmds, Sym(0));

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
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

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
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_eq!(tracer.segments.len(), 2);
    match &tracer.segments[1] {
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
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_eq!(tracer.segments.len(), 2);
    match &tracer.segments[1] {
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
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

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
    let tracer = PathTracerCallable::new(true, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(0.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_absolute_line() {
    // Simple absolute path: move to origin, line to (10, 0), line to (10, 10)
    let svg = "M 0 0 L 10 0 L 10 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(10.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_relative_line() {
    // Relative path: move to (5, 5), relative line +10 in x, then +10 in y
    let svg = "M 5 5 l 10 0 l 0 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(5.0, 5.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(15.0, 5.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(15.0, 15.0));
  }

  #[test]
  fn test_parse_svg_path_horizontal_vertical() {
    // H and V commands
    let svg = "M 0 0 H 10 V 10";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(0.5).unwrap(), Vec2::new(10.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 10.0));
  }

  #[test]
  fn test_parse_svg_path_cubic_bezier() {
    // Cubic bezier from (0,0) to (10,0) with control points
    let svg = "M 0 0 C 3 5, 7 5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_quadratic_bezier() {
    // Quadratic bezier from (0,0) to (10,0) with control point at (5, 5)
    let svg = "M 0 0 Q 5 5, 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_arc() {
    // Arc from (1,0) to (-1,0) with rx=ry=1
    let svg = "M 1 0 A 1 1 0 0 1 -1 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

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

    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));
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
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert!(matches!(tracer.segments[1], PathSegment::Cubic { .. }));
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_parse_svg_path_smooth_quadratic() {
    // Smooth quadratic: T command reflects the previous control point
    let svg = "M 0 0 Q 2.5 5, 5 0 T 10 0";
    let cmds = parse_svg_path_to_draw_commands(svg).unwrap();
    assert!(matches!(cmds[2], DrawCommand::SmoothQuadraticBezier { .. }));
    let tracer = PathTracerCallable::new(false, false, cmds, Sym(0));

    assert!(matches!(tracer.segments[1], PathSegment::Quadratic { .. }));
    assert_vec2_close(tracer.sample(0.0).unwrap(), Vec2::new(0.0, 0.0));
    assert_vec2_close(tracer.sample(1.0).unwrap(), Vec2::new(10.0, 0.0));
  }

  #[test]
  fn test_trace_path_alias_draw_commands() {
    let src = r#"
path = trace_path(|| {
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
}
