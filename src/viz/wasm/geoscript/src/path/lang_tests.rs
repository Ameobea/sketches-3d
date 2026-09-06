use std::hash::Hasher;

use crate::{parse_and_eval_program, EvalCtx, Value, Vec2};

fn v(ctx: &EvalCtx, name: &str) -> Vec2 {
  let val = ctx.get_global(name).unwrap();
  *val
    .as_vec2()
    .unwrap_or_else(|| panic!("{name} is not a vec2: {val:?}"))
}

fn f(ctx: &EvalCtx, name: &str) -> f32 {
  ctx.get_global(name).unwrap().as_float().unwrap()
}

fn close(a: Vec2, b: Vec2) -> bool {
  (a - b).norm() < 1e-4
}

fn subpath_count(ctx: &EvalCtx, name: &str) -> usize {
  ctx
    .get_global(name)
    .unwrap()
    .as_path()
    .unwrap()
    .leaves()
    .len()
}

#[test]
fn pen_chain_and_calling() {
  let ctx = parse_and_eval_program(
    r#"
p = path() | move(0, 0) | line(1, 0) | line(1, 1) | line(0, 1) | close
a = p(0)
b = p(0.25)
c = p(t=0.5)
l = len(p)
n = len(subpaths(p))
q = path() | line(0, 10) | line(5, 10)
q0 = q(0)
"#,
  )
  .unwrap();
  assert!(close(v(&ctx, "a"), Vec2::new(0., 0.)));
  assert!(close(v(&ctx, "b"), Vec2::new(1., 0.)));
  assert!(close(v(&ctx, "c"), Vec2::new(1., 1.)));
  assert!((f(&ctx, "l") - 4.).abs() < 1e-5);
  assert_eq!(ctx.get_global("n").unwrap().as_int().unwrap(), 1);
  assert!(close(v(&ctx, "q0"), Vec2::new(0., 0.)));
}

#[test]
fn constructors() {
  let ctx = parse_and_eval_program(
    r#"
c = circle(v2(0), 1)
c2 = circle(0, 0, 1)
r = rect(v2(1, 2), v2(4, 6))
r2 = rect(1, 2, 4, 6)
r3 = rect(v2(0), 2)
pg = polygon([v2(0, 0), v2(2, 0), v2(2, 2)])
pl = polyline([v2(0, 0), v2(3, 0), v2(3, 4)])
c0 = c(0)
c20 = c2(0.5)
r0 = r(0)
r20 = r2(0)
r30 = r3(0)
pg_len = len(pg)
pl_len = len(pl)
pl_end = pl(1)
"#,
  )
  .unwrap();
  assert!(close(v(&ctx, "c0"), Vec2::new(1., 0.)));
  assert!(close(v(&ctx, "c20"), Vec2::new(-1., 0.)));
  assert!(close(v(&ctx, "r0"), Vec2::new(3., 5.)));
  assert!(close(v(&ctx, "r20"), Vec2::new(3., 5.)));
  assert!(close(v(&ctx, "r30"), Vec2::new(1., 1.)));
  assert!((f(&ctx, "pg_len") - (4. + 8f32.sqrt())).abs() < 1e-4);
  assert!((f(&ctx, "pl_len") - 7.).abs() < 1e-5);
  assert!(close(v(&ctx, "pl_end"), Vec2::new(3., 4.)));
  assert!(parse_and_eval_program("polygon([v2(0, 0), v2(1, 0)])").is_err());
  assert!(parse_and_eval_program("polyline([v2(0, 0)])").is_err());
}

#[test]
fn grouping() {
  let ctx = parse_and_eval_program(
    r#"
g = path([circle(v2(0), 1), rect(v2(0), 4) | reverse], fill_rule='evenodd')
nested = path([nil, [circle(v2(0), 1)], circle(v2(5, 0), 1), path([rect(v2(9, 0), 1)])])
single = path(circle(v2(0), 1))
empty = path()
empty_list = path([])
seq_piped = [circle(v2(0), 1), circle(v2(3, 0), 1)] | path
empty_len = len(empty)
"#,
  )
  .unwrap();
  assert_eq!(subpath_count(&ctx, "g"), 2);
  assert_eq!(subpath_count(&ctx, "nested"), 3);
  assert_eq!(subpath_count(&ctx, "single"), 1);
  assert_eq!(subpath_count(&ctx, "empty"), 0);
  assert_eq!(subpath_count(&ctx, "empty_list"), 0);
  assert_eq!(subpath_count(&ctx, "seq_piped"), 2);
  assert_eq!(f(&ctx, "empty_len"), 0.);
  let g = ctx.get_global("g").unwrap();
  assert_eq!(
    g.as_path().unwrap().fill_rule,
    Some(super::FillRule::EvenOdd)
  );

  let err = parse_and_eval_program("p = path([|t| v2(t, 0)])").unwrap_err();
  assert!(format!("{err}").contains("path(f)"), "{err}");
  let err = parse_and_eval_program(
    "p = path([fill_rule('evenodd', circle(v2(0), 1)), fill_rule('nonzero', circle(v2(3, 0), 1))])",
  )
  .unwrap_err();
  assert!(format!("{err}").contains("conflicting fill rules"), "{err}");
}

#[test]
fn closure_shim() {
  let ctx = parse_and_eval_program(
    r#"
f = path(|t| v2(t * 10, 0))
a = f(0.5)
l = len(f)
ring = path(|t| v2(cos(t * tau), sin(t * tau)), closed=true)
ring_len = len(ring)
mixed = path([circle(v2(0), 1), f])
mixed_len = len(mixed)
"#,
  )
  .unwrap();
  assert!(close(v(&ctx, "a"), Vec2::new(5., 0.)));
  assert!((f(&ctx, "l") - 10.).abs() < 1e-3);
  assert!((f(&ctx, "ring_len") - std::f32::consts::TAU).abs() < 1e-2);
  assert!((f(&ctx, "mixed_len") - (std::f32::consts::TAU + 10.)).abs() < 1e-2);
  let err = parse_and_eval_program("p = path(|t| v2(t, 0)) | line(1, 1)").unwrap_err();
  assert!(format!("{err}").contains("discretize_path"), "{err}");
}

#[test]
fn whole_path_ops() {
  let ctx = parse_and_eval_program(
    r#"
r = rect(v2(1, 0), 2)
at0 = |p| p(0)
t1 = r | trans(v2(5, 0)) | at0
t2 = r | translate(5, 1) | at0
rt = r | rot(pi / 2) | at0
s1 = r | scale(2) | at0
s2 = r | scale(2, 3) | at0
s3 = r | scale(v2(2, 3)) | at0
rx = r | reflect_x | at0
rx2 = r | reflect_x(10) | at0
ry = r | reflect_y | at0
ry2 = r | reflect_y(10) | at0
rf = r | reflect(v2(1, 1)) | at0
rf2 = r | reflect(v2(1, 0), 4) | at0
rvp = r | reverse
rv = rvp(0.125)
og = rect(v2(5, 5), 2) | origin_to_geometry | at0
ev = fill_rule('evenodd', r)
rl = len(r)
"#,
  )
  .unwrap();
  assert!(close(v(&ctx, "t1"), Vec2::new(7., 1.)));
  assert!(close(v(&ctx, "t2"), Vec2::new(7., 2.)));
  assert!(close(v(&ctx, "rt"), Vec2::new(-1., 2.)));
  assert!(close(v(&ctx, "s1"), Vec2::new(4., 2.)));
  assert!(close(v(&ctx, "s2"), Vec2::new(4., 3.)));
  assert!(close(v(&ctx, "s3"), Vec2::new(4., 3.)));
  assert!(close(v(&ctx, "rx"), Vec2::new(2., -1.)));
  assert!(close(v(&ctx, "rx2"), Vec2::new(2., 19.)));
  assert!(close(v(&ctx, "ry"), Vec2::new(-2., 1.)));
  assert!(close(v(&ctx, "ry2"), Vec2::new(18., 1.)));
  assert!(close(v(&ctx, "rf"), Vec2::new(1., 2.)));
  assert!(close(v(&ctx, "rf2"), Vec2::new(2., 7.)));
  assert!(close(v(&ctx, "rv"), Vec2::new(2., 0.)));
  assert!(close(v(&ctx, "og"), Vec2::new(1., 1.)));
  assert_eq!(
    ctx.get_global("ev").unwrap().as_path().unwrap().fill_rule,
    Some(super::FillRule::EvenOdd)
  );
  assert!((f(&ctx, "rl") - 8.).abs() < 1e-5);
}

#[test]
fn every_pen_op_and_alias() {
  let ctx = parse_and_eval_program(
    r#"
p = path()
  | move(v2(0, 0))
  | quadratic_bezier(v2(1, 1), v2(2, 0))
  | smooth_quadratic_bezier(4, 0)
  | quad(v2(5, 1), v2(6, 0))
  | smooth_quad(v2(8, 0))
  | cubic_bezier(v2(8, 1), v2(9, 1), v2(10, 0))
  | smooth_cubic_bezier(v2(11, -1), v2(12, 0))
  | cubic(12, 1, 13, 1, 14, 0)
  | smooth_bezier(15, -1, 16, 0)
  | arc(1, 1, 0, false, true, v2(18, 0))
  | arc(1, 1, 0, true, false, 20, 0)
  | arc(1, 1, 0, v2(22, 0))
  | arc(1, 1, 0, 24, 0)
  | line(v2(24, 5))
  | close
p_end = p(1)
n = len(path_segments(p))
two = path() | move(0, 0) | line(1, 0) | move(5, 0) | line(6, 0) | close_all
two_closed = path_segments(two) -> |s| s.closed
after_closed = (circle(v2(0), 1) | line(3, 0))
ac_subs = subpaths(after_closed)
ac_n = len(ac_subs)
ac_second = ac_subs[1]
ac_start = ac_second(0)
"#,
  )
  .unwrap();
  assert!(close(v(&ctx, "p_end"), Vec2::new(0., 0.)));
  assert_eq!(ctx.get_global("n").unwrap().as_int().unwrap(), 14);
  let flags: Vec<bool> = ctx
    .get_global("two_closed")
    .unwrap()
    .as_sequence()
    .unwrap()
    .consume(&ctx)
    .map(|r| r.unwrap().as_bool().unwrap())
    .collect();
  assert_eq!(flags.len(), 4);
  assert!(flags.iter().all(|c| *c));
  assert_eq!(ctx.get_global("ac_n").unwrap().as_int().unwrap(), 2);
  assert!(close(v(&ctx, "ac_start"), Vec2::new(1., 0.)));
}

#[test]
fn path_type_hint() {
  let ctx = parse_and_eval_program(
    r#"
perim = |p: path| len(p)
c = perim(circle(v2(0), 1))
"#,
  )
  .unwrap();
  assert!((f(&ctx, "c") - std::f32::consts::TAU).abs() < 1e-2);
  assert!(parse_and_eval_program("perim = |p: path| len(p)\nperim(3)").is_err());
}

#[test]
fn grouping_flattens_to_the_same_hash() {
  let ctx = parse_and_eval_program(
    r#"
a = path([circle(v2(0), 1), rect(v2(0), v2(2, 1)) | reverse, path() | move(0, 5) | line(1, 1) | close])
b = path([circle(v2(0), 1), path([rect(v2(0), v2(2, 1)) | reverse, path() | move(0, 5) | line(1, 1) | close])])
"#,
  )
  .unwrap();
  let h = |name: &str| {
    let mut hasher = fxhash::FxHasher::default();
    assert!(ctx
      .get_global(name)
      .unwrap()
      .as_path()
      .unwrap()
      .content_hash(&mut hasher));
    hasher.finish()
  };
  assert_eq!(h("a"), h("b"));
}

#[test]
fn render_paths() {
  let ctx = parse_and_eval_program(
    r#"
render(circle(v2(0), 1))
render([circle(v2(0), 1), rect(v2(3, 0), 1)])
render(path([circle(v2(0), 1), rect(v2(3, 0), 1)]))
"#,
  )
  .unwrap();
  let rendered = ctx.rendered_paths.into_inner();
  assert_eq!(rendered.len(), 5);
  let first = &rendered[0].points;
  assert!((first[0] - first[first.len() - 1]).norm() < 1e-6);
}

#[test]
fn dropped_kwargs_are_rejected() {
  for src in [
    "tessellate_path(circle(v2(0), 1), closed=true)",
    "trace_svg_path('M 0 0 L 1 0', center=true)",
    "discretize_path(circle(v2(0), 1), closed=true)",
  ] {
    let err = parse_and_eval_program(src).unwrap_err();
    let _ = Value::Nil;
    assert!(format!("{err}").len() > 0, "{src}");
  }
}
