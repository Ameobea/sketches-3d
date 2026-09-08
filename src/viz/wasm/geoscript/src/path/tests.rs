use std::rc::Rc;

use nalgebra::Matrix3;

use super::{
  builder::{circle_subpath, polygon_subpath, polyline_subpath, rect_subpath},
  lazy::ClosurePath,
  DrawCommand as D, Path, PathKind, PathSegment,
};
use crate::{parse_and_eval_program, EvalCtx, Vec2};

fn v(x: f32, y: f32) -> Vec2 {
  Vec2::new(x, y)
}

fn close(a: Vec2, b: Vec2, eps: f32) -> bool {
  (a - b).norm() <= eps
}

fn rc(p: Path) -> Rc<Path> {
  Rc::new(p)
}

fn square() -> Rc<Path> {
  rc(Path::from_draw_commands(
    [
      D::MoveTo(v(0., 0.)),
      D::LineTo(v(1., 0.)),
      D::LineTo(v(1., 1.)),
      D::LineTo(v(0., 1.)),
      D::Close,
    ],
    false,
  ))
}

fn closure_path(_ctx: &EvalCtx, src: &str) -> Rc<Path> {
  let ctx_inner = parse_and_eval_program(&format!("f = {src}")).unwrap();
  let f = ctx_inner
    .get_global("f")
    .unwrap()
    .as_callable()
    .unwrap()
    .clone();
  rc(Path::lazy(Rc::new(ClosurePath { f, closed: None })))
}

#[test]
fn lazy_materialization_refines_curves_and_inflections() {
  let ctx = EvalCtx::default();
  let circle = closure_path(&ctx, r#"|t| vec2(cos(t * pi * 2), sin(t * pi * 2))"#);
  let coarse = circle
    .sample_subpaths_tagged(20f32.to_radians(), f32::INFINITY, 8, &ctx)
    .unwrap();
  let fine = circle
    .sample_subpaths_tagged(1f32.to_radians(), f32::INFINITY, 8, &ctx)
    .unwrap();
  assert!(fine[0].closed && coarse[0].closed);
  assert!(fine[0].points.len() > coarse[0].points.len() * 4);
  assert!(fine[0].points.len() > 64);
  assert!(fine[0].anchors.iter().all(|&a| !a));
  for i in 0..fine[0].points.len() {
    let mid = (fine[0].points[i] + fine[0].points[(i + 1) % fine[0].points.len()]) * 0.5;
    assert!(1. - mid.norm() < 0.00005);
  }
  let inflection = closure_path(&ctx, r#"|t| vec2(t, sin(t * pi * 2))"#);
  let sampled = inflection
    .sample_subpaths_tagged(5f32.to_radians(), f32::INFINITY, 2, &ctx)
    .unwrap();
  assert!(!sampled[0].closed);
  assert!(sampled[0].points.iter().any(|p| p.y > 0.99));
  assert!(sampled[0].points.iter().any(|p| p.y < -0.99));
  assert_eq!(
    *sampled[0].points.last().unwrap(),
    inflection.eval_at(1., &ctx).unwrap()
  );
  let sagged = circle
    .sample_subpaths_flat(180f32.to_radians(), 0.0001, 8, &ctx)
    .unwrap();
  assert!(
    sagged[0].0.len() > 100,
    "sagitta must also drive lazy refinement"
  );
}

#[test]
fn lazy_lerp_preserves_dense_structural_joints_without_promoting_them_to_creases() {
  use super::lazy::LerpPath;
  let ctx = EvalCtx::default();
  let polygon = |n: usize| {
    let points = (0..n)
      .map(|i| {
        let theta = i as f32 / n as f32 * std::f32::consts::TAU;
        let r = if i % 2 == 0 { 1. } else { 0.9 };
        v(theta.cos() * r, theta.sin() * r)
      })
      .collect();
    let flags = (0..n).map(|i| i % 17 == 0).collect();
    rc(Path::from_polylines(
      vec![(points, true)],
      Some(vec![flags]),
    ))
  };
  let a = polygon(257);
  let b = polygon(193);
  let p = rc(Path::lazy(Rc::new(LerpPath::new(a, b, 0.37, 64))));
  let sampled = p
    .sample_subpaths_tagged(5f32.to_radians(), f32::INFINITY, 8, &ctx)
    .unwrap();
  assert!(sampled[0].points.len() > 400);
  assert!(sampled[0].anchors.iter().filter(|&&a| a).count() < 40);
  let denser_seeds = p
    .sample_subpaths_tagged(1f32.to_radians(), f32::INFINITY, 4096, &ctx)
    .unwrap();
  assert_eq!(
    sampled[0].points, denser_seeds[0].points,
    "piecewise-linear interpolation is resolved by its joints, independently of probe density"
  );
  for t in p.sampling_t_values() {
    let expected = p.eval_at(t, &ctx).unwrap();
    assert!(sampled[0].points.iter().any(|&v| close(v, expected, 1e-6)));
  }
  for t in p.critical_t_values() {
    let expected = p.eval_at(t, &ctx).unwrap();
    assert!(sampled[0]
      .points
      .iter()
      .zip(&sampled[0].anchors)
      .any(|(&v, &a)| a && close(v, expected, 1e-6)));
  }
}

#[test]
fn discretization_retains_tags_through_mixed_groups_and_repeated_materialization() {
  let ctx = parse_and_eval_program(
    r#"
    a = circle(v2(0), 2)
    b = path() | move(0,0) | line(2,0) | line(0.73,1) | close
    p = path([discretize_path(a), lerp_paths(b,b,0.5)],fill_rule="evenodd") | reverse | scale(2,3)
    once = discretize_path(p, sample_count=8)
    twice = discretize_path(once)
  "#,
  )
  .unwrap();
  let once = ctx.get_global("once").unwrap().as_path().unwrap().clone();
  let twice = ctx.get_global("twice").unwrap().as_path().unwrap().clone();
  assert_eq!(once.leaves().len(), 2);
  assert_eq!(once.fill_rule, Some(super::FillRule::EvenOdd));
  assert_eq!(twice.fill_rule, once.fill_rule);
  assert_eq!(once.critical_t_values(), twice.critical_t_values());
  for (a, b) in once.leaves().iter().zip(twice.leaves()) {
    assert_eq!(a.anchors.as_slice(), b.anchors.as_slice());
    assert!(a.anchors.iter().filter(|&&flag| flag).count() <= 3);
  }
}

#[test]
fn lazy_materialization_obeys_global_angle_and_override() {
  let ctx = parse_and_eval_program(
    r#"
    set_curve_angle_threshold(15)
    p = path(|t| v2(cos(t*pi*2), sin(t*pi*2)))
    coarse = discretize_path(p, sample_count=8)
    fine = discretize_path(p, sample_count=8, curve_angle_degrees=1)
  "#,
  )
  .unwrap();
  let coarse = ctx.get_global("coarse").unwrap().as_path().unwrap().clone();
  let fine = ctx.get_global("fine").unwrap().as_path().unwrap().clone();
  assert!(fine.leaves()[0].segments.len() > coarse.leaves()[0].segments.len() * 4);
}

#[test]
fn lazy_materialization_rejects_nonfinite_geometry() {
  let ctx = EvalCtx::default();
  let p = closure_path(&ctx, r#"|t| vec2(t, sqrt(-1))"#);
  assert!(p.sample_subpaths(0.1, 8, &ctx).is_err());
}

#[test]
fn lazy_sampling_respects_transformed_groups_trimmed_joints_and_mesh_budgets() {
  use super::lazy::LerpPath;
  let ctx = EvalCtx::default();
  let ellipse = closure_path(&ctx, r#"|t| vec2(cos(t*pi*2),sin(t*pi*2))"#);
  let scale = Matrix3::new_nonuniform_scaling(&v(50., 1.));
  let direct = ellipse
    .transformed(&scale)
    .sample_subpaths(0.05, 8, &ctx)
    .unwrap();
  let group = Path::group_items(vec![ellipse.clone(), square()], None, &ctx)
    .unwrap()
    .transformed(&scale);
  let grouped = group.sample_subpaths(0.05, 8, &ctx).unwrap();
  assert_eq!(
    direct[0].0, grouped[0].0,
    "group transform must not weaken the angle tolerance"
  );
  let budgeted = ellipse
    .sample_subpaths_with_limit(0.01, Some(20), &ctx)
    .unwrap();
  assert_eq!(budgeted[0].0.len(), 20);
  let p = rc(Path::lazy(Rc::new(LerpPath::new(
    square(),
    square(),
    0.5,
    64,
  ))));
  let trimmed = rc(p.trimmed(0.125, 0.875)).reversed().transformed(&scale);
  assert!(trimmed.is_piecewise_linear());
  let sampled = trimmed
    .sample_subpaths_tagged(0.1, f32::INFINITY, 2, &ctx)
    .unwrap();
  assert!(!sampled[0].closed);
  for t in trimmed.critical_t_values() {
    let point = trimmed.eval_at(t, &ctx).unwrap();
    assert!(sampled[0]
      .points
      .iter()
      .zip(&sampled[0].anchors)
      .any(|(&p, &a)| a && close(p, point, 1e-6)));
  }
}

#[test]
fn pen_move_line_close() {
  let p = square();
  let leaves = p.leaves();
  assert_eq!(leaves.len(), 1);
  assert!(leaves[0].closed);
  assert_eq!(leaves[0].segments.len(), 4);
  let ctx = EvalCtx::default();
  assert!(close(p.eval_at(0., &ctx).unwrap(), v(0., 0.), 1e-6));
  assert!(close(p.eval_at(0.25, &ctx).unwrap(), v(1., 0.), 1e-6));
  assert!((p.length(&ctx).unwrap() - 4.).abs() < 1e-6);
}

#[test]
fn pen_after_closed_leaf_starts_new_leaf_at_its_start() {
  let c = rc(Path::leaf(circle_subpath(v(0., 0.), 1., false)));
  let p = c.pen(D::LineTo(v(2., 0.)), "line").unwrap();
  let leaves = p.leaves();
  assert_eq!(leaves.len(), 2);
  assert!(leaves[0].closed);
  assert!(!leaves[1].closed);
  assert!(close(leaves[1].start, v(1., 0.), 1e-6));
  assert!(close(leaves[1].end(), v(2., 0.), 1e-6));
}

#[test]
fn pen_on_empty_starts_at_origin_and_open_leaf_is_extended() {
  let p = rc(Path::empty());
  let p = rc(p.pen(D::LineTo(v(0., 10.)), "line").unwrap());
  let p = rc(p.pen(D::LineTo(v(5., 10.)), "line").unwrap());
  let leaves = p.leaves();
  assert_eq!(leaves.len(), 1);
  assert!(close(leaves[0].start, v(0., 0.), 1e-6));
  assert_eq!(leaves[0].segments.len(), 2);
  let closed = rc(p.pen(D::Close, "close").unwrap());
  assert!(closed.leaves()[0].closed);
  assert_eq!(closed.leaves()[0].segments.len(), 3);
  // close on a closed leaf is a no-op
  let again = closed.pen(D::Close, "close").unwrap();
  assert_eq!(again.leaves()[0].segments.len(), 3);
}

#[test]
fn pen_epsilon_and_bare_move() {
  let p = Path::from_draw_commands(
    [
      D::MoveTo(v(0., 0.)),
      D::LineTo(v(0., 0.)),
      D::MoveTo(v(5., 5.)),
      D::Close,
      D::MoveTo(v(1., 1.)),
      D::LineTo(v(2., 1.)),
    ],
    false,
  );
  let leaves = p.leaves();
  assert_eq!(leaves.len(), 3);
  assert!(leaves[0].segments.is_empty() && leaves[1].segments.is_empty());
  assert!(!leaves[1].closed);
  let ctx = EvalCtx::default();
  assert!(close(p.eval_at(0.5, &ctx).unwrap(), v(1.5, 1.), 1e-6));
  let polys = p.sample_subpaths(0.1, 64, &ctx).unwrap();
  assert_eq!(polys.iter().filter(|(pts, _)| pts.len() >= 2).count(), 1);
}

#[test]
fn pen_smooth_cubic_reflects_last_control() {
  let p = Path::from_draw_commands(
    [
      D::CubicBezier {
        ctrl1: v(1., 1.),
        ctrl2: v(2., 1.),
        to: v(3., 0.),
      },
      D::SmoothCubicBezier {
        ctrl2: v(5., -1.),
        to: v(6., 0.),
      },
    ],
    false,
  );
  match &p.leaves()[0].segments[1] {
    PathSegment::Cubic { ctrl1, .. } => assert!(close(*ctrl1, v(4., -1.), 1e-6)),
    other => panic!("{other:?}"),
  }
}

#[test]
fn close_all_closes_every_open_leaf() {
  let p = Path::from_draw_commands(
    [
      D::MoveTo(v(0., 0.)),
      D::LineTo(v(1., 0.)),
      D::LineTo(v(1., 1.)),
      D::MoveTo(v(5., 0.)),
      D::LineTo(v(6., 0.)),
      D::LineTo(v(6., 1.)),
    ],
    true,
  );
  let leaves = p.leaves();
  assert!(leaves.iter().all(|sp| sp.closed && sp.segments.len() == 3));
  assert_eq!(p.subpath_topology().unwrap().len(), 2);
}

#[test]
fn reverse_is_an_involution_and_bakes() {
  let p = rc(Path::from_draw_commands(
    [
      D::MoveTo(v(0., 0.)),
      D::LineTo(v(4., 0.)),
      D::QuadraticBezier {
        ctrl: v(5., 2.),
        to: v(4., 4.),
      },
      D::Arc {
        rx: 2.,
        ry: 2.,
        x_axis_rotation: 0.,
        large_arc: false,
        sweep: true,
        to: v(0., 4.),
      },
    ],
    false,
  ));
  let r = rc(p.reversed());
  assert!(r.is_concrete() && !r.reverse);
  let ctx = EvalCtx::default();
  for i in 0..=20 {
    let t = i as f32 / 20.;
    assert!(close(
      r.eval_at(t, &ctx).unwrap(),
      p.eval_at(1. - t, &ctx).unwrap(),
      1e-3
    ));
  }
  let rr = r.reversed();
  for i in 0..=20 {
    let t = i as f32 / 20.;
    assert!(close(
      rr.eval_at(t, &ctx).unwrap(),
      p.eval_at(t, &ctx).unwrap(),
      1e-3
    ));
  }
  let mut a: Vec<f32> = p.critical_t_values();
  let b = r.critical_t_values();
  a.iter_mut().for_each(|t| *t = 1. - *t);
  a.reverse();
  assert_eq!(a.len(), b.len());
  assert!(a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5));
}

#[test]
fn baked_rotation_matches_lazy_rotation() {
  let ctx = EvalCtx::default();
  let p = rc(Path::leaf(rect_subpath(v(1., 2.), 2., 4., false)));
  let m = Matrix3::new_rotation(0.7);
  let baked = p.transformed(&m);
  assert!(baked.is_concrete() && baked.transform == Matrix3::identity());
  let shim = closure_path(&ctx, "|t| vec2(0, 0)");
  let lazy_group = Path::group_items(vec![Rc::clone(&p), Rc::clone(&shim)], None, &ctx)
    .unwrap()
    .transformed(&m);
  assert!(!lazy_group.is_concrete());
  for i in 0..=10 {
    let t = i as f32 / 10. * 0.999;
    let expected = baked.eval_at(t, &ctx).unwrap();
    let got = lazy_group.eval_at(t, &ctx).unwrap();
    assert!(close(expected, got, 1e-4), "t={t}: {expected:?} vs {got:?}");
  }
}

#[test]
fn non_uniform_bake_converts_arcs_to_cubics() {
  let c = Path::leaf(circle_subpath(v(0., 0.), 1., false));
  let s = c.transformed(&Matrix3::new_nonuniform_scaling(&v(2., 1.)));
  let leaves = s.leaves();
  assert!(leaves[0]
    .segments
    .iter()
    .all(|seg| matches!(seg, PathSegment::Cubic { .. })));
  assert!(leaves[0].closed);
  let anchors = &leaves[0].anchors;
  assert_eq!(anchors.iter().filter(|&&a| a).count(), 2);
  let ctx = EvalCtx::default();
  let (min, max) = s.aabb(&ctx).unwrap().unwrap();
  assert!(close(min, v(-2., -1.), 2e-3) && close(max, v(2., 1.), 2e-3));
}

#[test]
fn group_items_flattens_and_resolves_fill_rules() {
  let ctx = EvalCtx::default();
  let a = rc(Path::leaf(circle_subpath(v(0., 0.), 1., false)));
  let b = rc(Path::leaf(rect_subpath(v(0., 0.), 1., 1., false)));
  let inner = rc(
    Path::group_items(
      vec![Rc::clone(&a), Rc::clone(&b)],
      Some(super::FillRule::EvenOdd),
      &ctx,
    )
    .unwrap(),
  );
  let outer = Path::group_items(vec![Rc::clone(&a), Rc::clone(&inner)], None, &ctx).unwrap();
  assert_eq!(outer.leaves().len(), 3);
  assert_eq!(outer.fill_rule, Some(super::FillRule::EvenOdd));
  let other = rc(inner.with_fill_rule(Some(super::FillRule::NonZero)));
  assert!(Path::group_items(vec![inner, other], None, &ctx).is_err());
  assert_eq!(outer.subpath_t_spans().unwrap().len(), 3);
  let single = Path::group_items(vec![Rc::clone(&a)], None, &ctx).unwrap();
  assert!(matches!(single.kind, PathKind::Group(_)));
}

#[test]
fn anchors_drive_critical_values() {
  let poly = vec![v(0., 0.), v(1., 0.), v(2., 0.), v(2., 2.)];
  let authored = Path::from_polylines(vec![(poly.clone(), false)], None);
  assert_eq!(authored.critical_t_values().len(), 4);
  let marked = Path::from_polylines(
    vec![(poly, true)],
    Some(vec![vec![false, false, true, true]]),
  );
  let cps = marked.critical_t_values();
  let total = marked.leaves()[0].total_length();
  assert_eq!(cps.len(), 2);
  assert!((cps[0] - 2. / total).abs() < 1e-6 && (cps[1] - 4. / total).abs() < 1e-6);
  let bounds = marked.leaves()[0].resample_boundaries();
  assert_eq!(bounds, vec![0., 2. / total, 4. / total, 1.]);
  assert_eq!(authored.leaves()[0].resample_boundaries(), vec![0., 1.]);
}

#[test]
fn centroids() {
  let ctx = EvalCtx::default();
  let r = Path::leaf(rect_subpath(v(3., 4.), 2., 6., false));
  assert!(close(r.centroid(&ctx).unwrap().unwrap(), v(3., 4.), 1e-5));
  let c = Path::leaf(circle_subpath(v(-2., 5.), 1.5, false));
  assert!(close(c.centroid(&ctx).unwrap().unwrap(), v(-2., 5.), 1e-4));
  assert!((c.leaves()[0].signed_area() - std::f32::consts::PI * 2.25).abs() < 1e-3);
  assert!(!c.leaves()[0].inward_flip());
  assert!(Path::leaf(circle_subpath(v(0., 0.), 1., true)).leaves()[0].inward_flip());

  let outer = rc(Path::leaf(rect_subpath(v(0., 0.), 4., 4., false)));
  let hole = rc(Path::leaf(rect_subpath(v(1., 0.), 1., 1., true)));
  let with_hole = Path::group_items(vec![outer, hole], None, &ctx).unwrap();
  let c = with_hole.centroid(&ctx).unwrap().unwrap();
  assert!(
    (c.x - (-1. / 15.)).abs() < 1e-5 && c.y.abs() < 1e-6,
    "{c:?}"
  );

  let open = Path::leaf(polyline_subpath(&[v(0., 0.), v(2., 0.), v(2., 1.)]).unwrap());
  assert!(close(
    open.centroid(&ctx).unwrap().unwrap(),
    v(4. / 3., 1. / 6.),
    1e-5
  ));
  let centered = open.origin_to_geometry(&ctx).unwrap();
  assert!(close(
    centered.centroid(&ctx).unwrap().unwrap(),
    v(0., 0.),
    1e-5
  ));
}

#[test]
fn empty_path_behaviors() {
  let ctx = EvalCtx::default();
  let e = rc(Path::empty());
  assert!(e.eval_at(0.5, &ctx).is_err());
  assert_eq!(e.length(&ctx).unwrap(), 0.);
  assert!(e.critical_t_values().is_empty());
  assert!(e.centroid(&ctx).unwrap().is_none());
  assert!(e.origin_to_geometry(&ctx).is_ok());
  assert!(e.subpaths().is_empty());
  assert!(e.sample_subpaths(0.1, 64, &ctx).unwrap().is_empty());
  assert!(e.subpath_t_spans().is_none());
}

#[test]
fn trim_preserves_corners_and_lazy_trim_remaps() {
  let ctx = EvalCtx::default();
  let sq = square();
  let half = sq.trimmed(0., 0.5);
  let leaves = half.leaves();
  assert_eq!(leaves.len(), 1);
  assert!(!leaves[0].closed);
  assert_eq!(leaves[0].segments.len(), 2);
  assert!(close(half.eval_at(0., &ctx).unwrap(), v(0., 0.), 1e-6));
  assert!(close(half.eval_at(0.5, &ctx).unwrap(), v(1., 0.), 1e-6));
  assert!(close(half.eval_at(1., &ctx).unwrap(), v(1., 1.), 1e-6));
  let mid = sq.trimmed(0.125, 0.375);
  assert_eq!(mid.critical_t_values().len(), 3);

  let f = closure_path(&ctx, "|t| vec2(t * 10, 0)");
  let t = f.trimmed(0.2, 0.7);
  assert!(!t.is_concrete());
  assert!(close(t.eval_at(0., &ctx).unwrap(), v(2., 0.), 1e-5));
  assert!(close(t.eval_at(1., &ctx).unwrap(), v(7., 0.), 1e-5));
}

#[test]
fn lazy_group_length_and_eval() {
  let ctx = EvalCtx::default();
  let sq = square();
  let f = closure_path(&ctx, "|t| vec2(t * 4, -1)");
  let g = rc(Path::group_items(vec![Rc::clone(&sq), Rc::clone(&f)], None, &ctx).unwrap());
  assert!(!g.is_concrete());
  assert!((g.length(&ctx).unwrap() - 8.).abs() < 1e-3);
  assert!(close(g.eval_at(0.25, &ctx).unwrap(), v(1., 1.), 1e-4));
  assert!(close(g.eval_at(0.75, &ctx).unwrap(), v(2., -1.), 1e-3));
  assert_eq!(g.subpath_t_spans().unwrap().len(), 2);
  let cps = g.critical_t_values();
  assert_eq!(cps.len(), 5);
  assert!((cps[0]).abs() < 1e-6 && (cps.last().unwrap() - 0.5).abs() < 1e-3);
  let polys = g.sample_subpaths(0.1, 8, &ctx).unwrap();
  assert_eq!(polys.len(), 2);
  assert!(polys[0].1 && !polys[1].1);
  let kids = g.subpaths();
  assert_eq!(kids.len(), 2);
  let rev = rc(g.reversed());
  assert!(rev.reverse);
  assert!(close(rev.eval_at(0.25, &ctx).unwrap(), v(2., -1.), 1e-3));
  assert_eq!(rev.subpaths().len(), 2);
  assert!(rev.subpaths()[0].is_concrete() == false);
}

#[test]
fn content_hash_is_structural() {
  use std::hash::Hasher;
  let h = |p: &Path| {
    let mut hasher = fxhash::FxHasher::default();
    assert!(p.content_hash(&mut hasher));
    hasher.finish()
  };
  assert_eq!(h(&square()), h(&square()));
  let poly = Path::leaf(polygon_subpath(&[v(0., 0.), v(1., 0.), v(1., 1.), v(0., 1.)]).unwrap());
  assert_ne!(h(&square()), h(&poly));
  assert_ne!(h(&square()), h(&square().reversed()));
}

#[test]
fn polygon_and_polyline_validation() {
  assert!(polygon_subpath(&[v(0., 0.), v(1., 0.)]).is_err());
  assert!(polygon_subpath(&[v(0., 0.), v(0., 0.), v(1., 0.), v(1., 1.)]).is_ok());
  assert!(polyline_subpath(&[v(0., 0.)]).is_err());
  let sp = polygon_subpath(&[v(0., 0.), v(2., 0.), v(2., 2.)]).unwrap();
  assert!(sp.closed && sp.segments.len() == 3);
}
