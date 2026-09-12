//! `simplify_path`: Ramer–Douglas–Peucker over the straight runs of each subpath. Curve
//! segments and producer-marked anchors are fixed, so critical points survive; authored joints
//! (pen ops, `polygon`) are all fair game.
use std::rc::Rc;

use fxhash::FxHashMap;

use crate::{
  builtins::trace_path::{discretize_path, expect_path},
  path::{line_segment, Path, PathSegment, Subpath},
  ArgRef, ErrorStack, EvalCtx, Sym, Value, Vec2,
};

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f32 {
  let ab = b - a;
  let len_sq = ab.norm_squared();
  if len_sq <= 1e-24 {
    return (p - a).norm();
  }
  let t = ((p - a).dot(&ab) / len_sq).clamp(0., 1.);
  (p - (a + ab * t)).norm()
}

/// Keeps the joints of the chain `idx` (endpoints already kept) needed so that every dropped
/// joint lies within `tol` of the segment between its kept neighbors.
fn rdp(pts: &[Vec2], idx: &[usize], tol: f32, keep: &mut [bool]) {
  let mut stack = vec![(0, idx.len() - 1)];
  while let Some((a, b)) = stack.pop() {
    if b <= a + 1 {
      continue;
    }
    let (pa, pb) = (pts[idx[a]], pts[idx[b]]);
    let (mut best, mut best_d) = (a, -1.);
    for i in a + 1..b {
      let d = dist_to_segment(pts[idx[i]], pa, pb);
      if d > best_d {
        (best, best_d) = (i, d);
      }
    }
    if best_d > tol {
      keep[idx[best]] = true;
      stack.push((a, best));
      stack.push((best, b));
    }
  }
}

pub(crate) fn simplify_subpath(sp: &Subpath, tol: f32) -> Subpath {
  let segs = sp.segments.as_slice();
  let n = segs.len();
  if n == 0 {
    return sp.clone();
  }
  let closed = sp.closed;
  let n_joints = if closed { n } else { n + 1 };
  let pts: Vec<Vec2> = (0..n_joints)
    .map(|j| {
      if j < n {
        segs[j].start_point()
      } else {
        segs[n - 1].end()
      }
    })
    .collect();
  let is_line = |s: usize| matches!(segs[s], PathSegment::Line { .. });
  // Joint `j` starts segment `j`; the segment ending at it is `j - 1` (wrapping when closed).
  let mut keep: Vec<bool> = (0..n_joints)
    .map(|j| {
      let ends_curve = match j {
        0 if closed => !is_line(n - 1),
        0 => true,
        _ => !is_line(j - 1),
      };
      let starts_curve = j >= n || !is_line(j);
      ends_curve || starts_curve || (sp.explicit_anchors && sp.anchors[j])
    })
    .collect();
  let farthest = |from: &dyn Fn(usize) -> f32, keep: &[bool]| {
    (0..n)
      .filter(|&j| !keep[j])
      .max_by(|&a, &b| from(a).total_cmp(&from(b)))
  };
  // A loop needs two fixed joints to split into chains, and three to stay a polygon.
  if closed && keep.iter().filter(|&&k| k).count() < 2 {
    let f0 = keep.iter().position(|&k| k).unwrap_or(0);
    keep[f0] = true;
    if let Some(far) = farthest(&|j| (pts[j] - pts[f0]).norm_squared(), &keep) {
      keep[far] = true;
    }
  }
  let order: Vec<usize> = if closed {
    let r = keep.iter().position(|&k| k).unwrap();
    (0..=n).map(|i| (r + i) % n).collect()
  } else {
    (0..n_joints).collect()
  };
  let mut chain_start = 0;
  for i in 1..order.len() {
    if keep[order[i]] {
      rdp(&pts, &order[chain_start..=i], tol, &mut keep);
      chain_start = i;
    }
  }
  if closed && n >= 3 && keep.iter().filter(|&&k| k).count() < 3 {
    let ks: Vec<usize> = (0..n).filter(|&j| keep[j]).collect();
    let (a, b) = (pts[ks[0]], pts[ks[1]]);
    if let Some(far) = farthest(&|j| dist_to_segment(pts[j], a, b), &keep) {
      keep[far] = true;
    }
  }

  // Keep the authored start (t = 0) whenever it survives.
  let r = if keep[0] { 0 } else { order[0] };
  let (mut out_segs, mut out_anchors) = (Vec::new(), Vec::new());
  let mut last = r;
  for i in 0..n {
    let j = (r + i) % n;
    let next = if closed { (j + 1) % n } else { j + 1 };
    match &segs[j] {
      PathSegment::Line { .. } => {
        if keep[next] {
          out_segs.push(line_segment(pts[last], pts[next]));
          out_anchors.push(sp.anchors[last]);
          last = next;
        }
      }
      seg => {
        debug_assert_eq!(last, j);
        out_segs.push(seg.clone());
        out_anchors.push(sp.anchors[j]);
        last = next;
      }
    }
  }
  Subpath::new(
    pts[r],
    out_segs,
    closed,
    out_anchors,
    sp.explicit_anchors,
    None,
  )
}

pub(crate) fn simplify_path(path: &Path, tol: f32) -> Path {
  let leaves = path
    .leaves()
    .into_iter()
    .map(|sp| Rc::new(Path::leaf(simplify_subpath(sp, tol))))
    .collect();
  Path::concrete_group(leaves, path.fill_rule)
}

pub fn simplify_path_impl(
  ctx: &EvalCtx,
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let (path_ix, tol_ix) = if def_ix == 0 { (0, 1) } else { (1, 0) };
  let path = expect_path(arg_refs[path_ix].resolve(args, kwargs), "simplify_path")?;
  let tol_val = arg_refs[tol_ix].resolve(args, kwargs);
  let tol = tol_val
    .as_float()
    .filter(|t| t.is_finite() && *t >= 0.)
    .ok_or_else(|| {
      ErrorStack::new(format!(
        "Invalid tolerance for `simplify_path`; expected a finite number >= 0, found: {tol_val:?}"
      ))
    })?;
  let discretized;
  let path: &Path = if path.first_lazy_name().is_none() {
    path
  } else {
    let angle = ctx.resolve_curve_angle_degrees(&Value::Nil).to_radians();
    discretized = discretize_path(ctx, path, angle, 128, None)?;
    &discretized
  };
  Ok(Value::Path(Rc::new(simplify_path(path, tol))))
}

#[cfg(test)]
mod tests {
  use std::f32::consts::PI;

  use super::*;
  use crate::{parse_and_eval_program, path::DrawCommand as D};

  fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
  }

  fn segment_count(p: &Path) -> usize {
    p.leaves().iter().map(|sp| sp.segments.len()).sum()
  }

  #[test]
  fn drops_detail_within_tolerance_and_keeps_curves_and_anchors() {
    // Authored joints: the collinear one goes, the quadratic and its endpoints stay verbatim.
    let p = Path::from_draw_commands(
      [
        D::MoveTo(v(0., 0.)),
        D::LineTo(v(1., 0.)),
        D::LineTo(v(2., 0.001)),
        D::LineTo(v(3., 0.)),
        D::QuadraticBezier {
          ctrl: v(4., 2.),
          to: v(5., 0.),
        },
        D::LineTo(v(5., 3.)),
        D::Close,
      ],
      false,
    );
    let s = simplify_path(&p, 0.01);
    let segs = s.leaves()[0].segments.as_slice().to_vec();
    assert_eq!(segs.len(), 4, "{segs:?}");
    assert!(matches!(segs[1], PathSegment::Quadratic { .. }));
    assert_eq!(segs[0].start_point(), v(0., 0.));
    assert_eq!(segs[0].end(), v(3., 0.));

    // Producer-marked anchor on a collinear joint pins it.
    let pts = vec![v(0., 0.), v(1., 0.), v(2., 0.), v(3., 0.), v(3., 1.)];
    let flags = vec![true, false, true, false, true];
    let p = Path::from_polylines(vec![(pts.clone(), false)], Some(vec![flags]));
    let s = simplify_path(&p, 0.);
    let joints: Vec<Vec2> = s.leaves()[0]
      .segments
      .iter()
      .map(|s| s.start_point())
      .collect();
    assert_eq!(joints, vec![v(0., 0.), v(2., 0.), v(3., 0.)]);
    assert_eq!(s.leaves()[0].anchors.as_slice(), &[true, true, false]);
    let p = Path::from_polylines(vec![(pts, false)], None);
    assert_eq!(segment_count(&simplify_path(&p, 0.)), 2);
  }

  #[test]
  fn closed_loops_stay_polygons_within_tolerance() {
    let n = 400;
    let circle: Vec<Vec2> = (0..n)
      .map(|i| {
        let a = i as f32 / n as f32 * 2. * PI;
        v(a.cos(), a.sin())
      })
      .collect();
    let p = Path::from_polylines(vec![(circle.clone(), true)], None);
    for (tol, lo, hi) in [(0.01, 12, 40), (0.1, 5, 12), (10., 3, 3)] {
      let s = simplify_path(&p, tol);
      let sp = &s.leaves()[0];
      assert!(sp.closed);
      let count = sp.segments.len();
      assert!((lo..=hi).contains(&count), "tol {tol}: {count} segments");
      let segs = sp.segments.as_slice();
      for c in &circle {
        let d = segs
          .iter()
          .map(|s| dist_to_segment(*c, s.start_point(), s.end()))
          .fold(f32::INFINITY, f32::min);
        assert!(
          d <= tol.min(1.) + 1e-5,
          "tol {tol}: point {c:?} is {d} away"
        );
      }
    }
  }

  #[test]
  fn builtin_forms_and_lazy_input() {
    let src = r#"
pts = 0..360 -> |i| { a = i / 360 * pi * 2; v2(cos(a), sin(a)) * 5 }
p = polygon(pts)
n_full = len(path_segments(p))
n_pos = len(path_segments(simplify_path(p, 0.05)))
n_pipe = len(path_segments(p | simplify_path(0.05)))
n_kw = len(path_segments(p | simplify_path(tolerance=0.05)))
n_swap = len(path_segments(simplify_path(0.05, p)))
n_default = len(path_segments(p | simplify_path))
c = catmull_rom(0..36 -> |i| { a = i / 36 * pi * 2; v2(cos(a), sin(a)) * 5 }, 0.5, true)
n_lerp_raw = len(path_segments(discretize_path(lerp_paths(c, c | scale(2, 2), 0.5))))
n_lerp = len(path_segments(lerp_paths(c, c | scale(2, 2), 0.5) | simplify_path(0.05)))
"#;
    let ctx = parse_and_eval_program(src).unwrap();
    let int = |name: &str| ctx.get_global(name).unwrap().as_int().unwrap();
    assert_eq!(int("n_full"), 360);
    assert!(int("n_pos") < 120 && int("n_pos") > 10, "{}", int("n_pos"));
    assert_eq!(int("n_pipe"), int("n_pos"));
    assert_eq!(int("n_kw"), int("n_pos"));
    assert_eq!(int("n_swap"), int("n_pos"));
    assert!(int("n_default") > int("n_pos") && int("n_default") < 360);
    assert!(
      int("n_lerp") > 8 && int("n_lerp") < int("n_lerp_raw") / 2,
      "{} of {}",
      int("n_lerp"),
      int("n_lerp_raw")
    );
    assert!(
      parse_and_eval_program("p = polygon([v2(0,0), v2(1,0), v2(0,1)]) | simplify_path(-1)")
        .is_err()
    );
  }
}
