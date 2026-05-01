use std::ops::{Add, Mul, Neg, Sub};
use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::linked_mesh::Vec3;

use crate::{seq::EagerSeq, ArgRef, ErrorStack, EvalCtx, Sym, Value, Vec2, EMPTY_KWARGS};

trait FilletVec:
  Copy
  + Add<Output = Self>
  + Sub<Output = Self>
  + Mul<f32, Output = Self>
  + Neg<Output = Self>
  + 'static
{
  fn vdot(self, other: Self) -> f32;
  fn vnorm(self) -> f32;
  fn into_value(self) -> Value;
  fn from_value(v: &Value) -> Option<Self>;
  fn type_name() -> &'static str;
}

impl FilletVec for Vec2 {
  #[inline]
  fn vdot(self, other: Self) -> f32 {
    nalgebra::Matrix::dot(&self, &other)
  }
  #[inline]
  fn vnorm(self) -> f32 {
    nalgebra::Matrix::norm(&self)
  }
  fn into_value(self) -> Value {
    Value::Vec2(self)
  }
  fn from_value(v: &Value) -> Option<Self> {
    v.as_vec2().copied()
  }
  fn type_name() -> &'static str {
    "vec2"
  }
}

impl FilletVec for Vec3 {
  #[inline]
  fn vdot(self, other: Self) -> f32 {
    nalgebra::Matrix::dot(&self, &other)
  }
  #[inline]
  fn vnorm(self) -> f32 {
    nalgebra::Matrix::norm(&self)
  }
  fn into_value(self) -> Value {
    Value::Vec3(self)
  }
  fn from_value(v: &Value) -> Option<Self> {
    v.as_vec3().copied()
  }
  fn type_name() -> &'static str {
    "vec3"
  }
}

const COLLINEAR_EPS: f32 = 1e-4;
const ZERO_LEN_EPS: f32 = 1e-6;

fn fillet_polyline<V: FilletVec>(
  points: &[V],
  resolution: usize,
  closed: bool,
  clamp_radius: bool,
  mut get_radius: impl FnMut(usize, V) -> Result<f32, ErrorStack>,
) -> Result<Vec<V>, ErrorStack> {
  let n = points.len();
  if n < 3 {
    return Ok(points.to_vec());
  }

  let resolution = resolution.max(1);
  let mut out: Vec<V> = Vec::with_capacity(n + n * resolution);

  let corner_indices: Vec<(usize, usize, usize)> = if closed {
    (0..n).map(|i| ((i + n - 1) % n, i, (i + 1) % n)).collect()
  } else {
    (1..n - 1).map(|i| (i - 1, i, i + 1)).collect()
  };

  if !closed {
    out.push(points[0]);
  }

  for &(prev_ix, b_ix, next_ix) in &corner_indices {
    let a = points[prev_ix];
    let b = points[b_ix];
    let c = points[next_ix];

    let to_in = b - a;
    let to_out = c - b;
    let l_in = to_in.vnorm();
    let l_out = to_out.vnorm();

    if l_in < ZERO_LEN_EPS || l_out < ZERO_LEN_EPS {
      out.push(b);
      continue;
    }

    let d_in = to_in * (1. / l_in);
    let d_out = to_out * (1. / l_out);

    let cos_alpha = d_in.vdot(d_out).clamp(-1., 1.);
    let alpha = cos_alpha.acos();

    if alpha < COLLINEAR_EPS || alpha > std::f32::consts::PI - COLLINEAR_EPS {
      out.push(b);
      continue;
    }

    let user_radius = get_radius(b_ix, b)?;
    if !user_radius.is_finite() || user_radius <= ZERO_LEN_EPS {
      out.push(b);
      continue;
    }

    let half_alpha = alpha * 0.5;
    let tan_half = half_alpha.tan();

    let max_t = l_in.min(l_out) * 0.5;
    let max_radius = max_t / tan_half;

    let radius = if clamp_radius {
      user_radius.min(max_radius)
    } else if user_radius > max_radius {
      return Err(ErrorStack::new(format!(
        "fillet_path: radius {user_radius} too large for corner at index {b_ix} (max safe \
         radius for this corner is ~{max_radius:.6}); reduce radius or pass clamp_radius=true"
      )));
    } else {
      user_radius
    };

    if radius <= ZERO_LEN_EPS {
      out.push(b);
      continue;
    }

    let t = radius * tan_half;
    let p1 = b + d_in * (-t);
    let p2 = b + d_out * t;

    // perpendicular to d_in in the bend plane, pointing toward arc center
    let perp = d_out + d_in * (-d_in.vdot(d_out));
    let perp_len = perp.vnorm();
    if perp_len < ZERO_LEN_EPS {
      out.push(b);
      continue;
    }
    let n1 = perp * (1. / perp_len);
    let arc_center = p1 + n1 * radius;

    let v1 = p1 - arc_center;
    let v2 = p2 - arc_center;
    let sin_alpha = alpha.sin();

    out.push(p1);
    for k in 1..resolution {
      let s = k as f32 / resolution as f32;
      let w1 = ((1. - s) * alpha).sin() / sin_alpha;
      let w2 = (s * alpha).sin() / sin_alpha;
      out.push(arc_center + v1 * w1 + v2 * w2);
    }
    out.push(p2);
  }

  if !closed {
    out.push(points[n - 1]);
  }

  Ok(out)
}

fn collect_points<V: FilletVec>(
  ctx: &EvalCtx,
  seq: &Rc<dyn crate::Sequence>,
  fn_name: &str,
) -> Result<Vec<V>, ErrorStack> {
  seq
    .consume(ctx)
    .map(|res| match res {
      Ok(val) => V::from_value(&val).ok_or_else(|| {
        ErrorStack::new(format!(
          "{fn_name}: expected sequence of {} points, found: {val:?}",
          V::type_name()
        ))
      }),
      Err(err) => Err(err),
    })
    .collect()
}

fn build_radius_callback<'a, V: FilletVec>(
  ctx: &'a EvalCtx,
  radius_arg: &'a Value,
  fn_name: &'static str,
) -> Result<Box<dyn FnMut(usize, V) -> Result<f32, ErrorStack> + 'a>, ErrorStack> {
  if let Some(f) = radius_arg.as_float() {
    Ok(Box::new(move |_ix, _v| Ok(f)))
  } else if let Some(cb) = radius_arg.as_callable() {
    let cb = Rc::clone(cb);
    Ok(Box::new(move |ix, v| {
      let result = ctx
        .invoke_callable(
          &cb,
          &[Value::Int(ix as i64), v.into_value()],
          EMPTY_KWARGS,
        )
        .map_err(|err| {
          err.wrap(format!(
            "Error calling user-provided cb passed to `radius` arg in `{fn_name}`"
          ))
        })?;
      result.as_float().ok_or_else(|| {
        ErrorStack::new(format!(
          "Expected Float from user-provided cb passed to `radius` arg in `{fn_name}`, \
           found: {result:?}"
        ))
      })
    }))
  } else {
    Err(ErrorStack::new(format!(
      "{fn_name}: `radius` must be a number or callable, found: {radius_arg:?}"
    )))
  }
}

fn fillet_path_generic<V: FilletVec>(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
  fn_name: &'static str,
) -> Result<Value, ErrorStack> {
  let path_seq = arg_refs[0]
    .resolve(args, kwargs)
    .as_sequence()
    .ok_or_else(|| ErrorStack::new(format!("{fn_name}: `path` must be a sequence")))?;
  let radius_arg = arg_refs[1].resolve(args, kwargs);
  let resolution = arg_refs[2]
    .resolve(args, kwargs)
    .as_int()
    .ok_or_else(|| ErrorStack::new(format!("{fn_name}: `resolution` must be an integer")))?
    as usize;
  let clamp_radius = arg_refs[3]
    .resolve(args, kwargs)
    .as_bool()
    .ok_or_else(|| ErrorStack::new(format!("{fn_name}: `clamp_radius` must be a bool")))?;
  let closed = arg_refs[4]
    .resolve(args, kwargs)
    .as_bool()
    .ok_or_else(|| ErrorStack::new(format!("{fn_name}: `closed` must be a bool")))?;

  let points: Vec<V> = collect_points(ctx, &path_seq, fn_name)?;
  let get_radius = build_radius_callback::<V>(ctx, radius_arg, fn_name)?;
  let out = fillet_polyline(&points, resolution, closed, clamp_radius, get_radius)?;
  let values: Vec<Value> = out.into_iter().map(V::into_value).collect();
  Ok(Value::Sequence(Rc::new(EagerSeq { inner: values })))
}

pub fn fillet_path_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  fillet_path_generic::<Vec2>(ctx, arg_refs, args, kwargs, "fillet_path")
}

pub fn fillet_path_3d_impl(
  ctx: &EvalCtx,
  _def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  fillet_path_generic::<Vec3>(ctx, arg_refs, args, kwargs, "fillet_path_3d")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::parse_and_eval_program;

  fn approx_eq(a: f32, b: f32, eps: f32) -> bool {
    (a - b).abs() < eps
  }

  #[test]
  fn fillet_2d_right_angle_uses_circular_arc() {
    // L-shape: (0,0) -> (1,0) -> (1,1). 90 deg corner.
    let pts = vec![
      Vec2::new(0., 0.),
      Vec2::new(1., 0.),
      Vec2::new(1., 1.),
    ];
    let r = 0.25;
    let out = fillet_polyline(&pts, 16, false, false, |_, _| Ok(r)).unwrap();

    // first and last points untouched
    assert!((out[0] - pts[0]).vnorm() < 1e-5);
    assert!((out[out.len() - 1] - pts[2]).vnorm() < 1e-5);

    // For a 90-deg fillet of radius r, the arc center is at (1-r, r).
    let center = Vec2::new(1. - r, r);
    // every arc point should be exactly r from the center
    for (i, p) in out.iter().enumerate() {
      if i == 0 || i == out.len() - 1 {
        continue;
      }
      let d = (*p - center).vnorm();
      assert!(approx_eq(d, r, 1e-4), "arc point {i} dist {d} != {r}");
    }
  }

  #[test]
  fn fillet_3d_short_path_passes_through() {
    let pts = vec![Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.)];
    let out = fillet_polyline(&pts, 8, false, true, |_, _| Ok(1.0)).unwrap();
    assert_eq!(out.len(), 2);
  }

  #[test]
  fn fillet_3d_collinear_passes_through() {
    let pts = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(2., 0., 0.),
    ];
    let out = fillet_polyline(&pts, 8, false, true, |_, _| Ok(0.5)).unwrap();
    // 3 input points -> 3 output points: start, middle (passed through, collinear), end
    assert_eq!(out.len(), 3);
  }

  #[test]
  fn fillet_3d_clamp_caps_radius() {
    // L corner with adjacent segments of length 0.1 each; radius 10 would otherwise overshoot.
    let pts = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0.1, 0., 0.),
      Vec3::new(0.1, 0.1, 0.),
    ];
    let res = 8;
    let out = fillet_polyline(&pts, res, false, true, |_, _| Ok(10.0)).unwrap();
    // With clamp_radius=true, the radius is shrunk so the tangent setback equals
    // half the shortest segment (0.05) for a 90-deg corner -> radius = 0.05.
    let expected_r = 0.05;
    let center = Vec3::new(0.1 - expected_r, expected_r, 0.);
    for p in out.iter().skip(1).take(out.len() - 2) {
      assert!(approx_eq((*p - center).vnorm(), expected_r, 1e-4));
    }
  }

  #[test]
  fn fillet_3d_unclamped_errors_when_too_large() {
    let pts = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(0.1, 0., 0.),
      Vec3::new(0.1, 0.1, 0.),
    ];
    let result = fillet_polyline(&pts, 8, false, false, |_, _| Ok(10.0));
    assert!(result.is_err());
  }

  #[test]
  fn fillet_3d_closed_square() {
    // A unit square traced clockwise; with closed=true every corner is filleted.
    let pts = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(1., 1., 0.),
      Vec3::new(0., 1., 0.),
    ];
    let r = 0.1;
    let res = 4;
    let out = fillet_polyline(&pts, res, true, false, |_, _| Ok(r)).unwrap();
    // Each of 4 corners produces (res + 1) points -> 4 * 5 = 20.
    assert_eq!(out.len(), 4 * (res + 1));
  }

  #[test]
  fn integration_fillet_path_3d_runs_through_eval() {
    let src = r#"
out = [v3(0,0,0), v3(1,0,0), v3(1,1,0), v3(1,1,1)]
  | fillet_path_3d(radius=0.2, resolution=4)
  | collect
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let val = ctx.get_global("out").unwrap();
    let seq = val.as_sequence().unwrap();
    let count = seq.consume(&ctx).count();
    // 4 input points, 2 interior corners filleted into 5 points each, plus the 2 endpoints
    // -> 1 + 5 + 5 + 1 = 12.
    assert_eq!(count, 12);
  }

  #[test]
  fn integration_fillet_path_2d_runs_through_eval() {
    let src = r#"
out = [v2(0,0), v2(1,0), v2(1,1)]
  | fillet_path(radius=0.25, resolution=8)
  | collect
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let val = ctx.get_global("out").unwrap();
    let seq = val.as_sequence().unwrap();
    let count = seq.consume(&ctx).count();
    // 1 (start) + 9 (arc) + 1 (end) = 11
    assert_eq!(count, 11);
  }

  #[test]
  fn integration_fillet_dynamic_radius_callable() {
    let src = r#"
out = [v3(0,0,0), v3(1,0,0), v3(1,1,0)]
  | fillet_path_3d(radius=|i: int, p: vec3| 0.1, resolution=4)
  | collect
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let val = ctx.get_global("out").unwrap();
    let seq = val.as_sequence().unwrap();
    let count = seq.consume(&ctx).count();
    assert_eq!(count, 7);
  }
}
