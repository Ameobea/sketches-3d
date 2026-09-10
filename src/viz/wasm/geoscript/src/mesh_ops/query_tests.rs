use mesh::linked_mesh::Vec3;

use crate::{parse_and_eval_program, Value};

fn bools(src: &str, names: &[(&str, bool)]) {
  let ctx = parse_and_eval_program(src).unwrap();
  for (name, want) in names {
    let got = ctx.get_global(name).unwrap().as_bool().unwrap();
    assert_eq!(got, *want, "{name}");
  }
}

#[test]
fn intersects_semantics() {
  bools(
    r#"
c = 0.5 + 0.70710678
overlap = intersects(box(1), box(1) | trans(0.5, 0, 0))
sep = intersects(box(1), box(1) | trans(2, 0, 0))
face = intersects(box(1), box(1) | trans(1, 0, 0))
face_gap = intersects(box(1), box(1) | trans(1.00001, 0, 0))
edge = intersects(box(1), box(1) | trans(1, 1, 0))
corner = intersects(box(1), box(1) | trans(1, 1, 1))
inside = intersects(box(1), box(0.5))
vtx_face = intersects(box(1), box(1) | rot(0, 0, pi/4) | trans(c, 0, 0))
vtx_face_gap = intersects(box(1), box(1) | rot(0, 0, pi/4) | trans(c + 0.0001, 0, 0))
vtx_face_over = intersects(box(1), box(1) | rot(0, 0, pi/4) | trans(c - 0.0001, 0, 0))
slide = intersects(box(1), box(1) | trans(1, 0.5, 0))
empty = intersects(box(1), mesh())
"#,
    &[
      ("overlap", true),
      ("sep", false),
      ("face", true),
      ("face_gap", false),
      ("edge", true),
      ("corner", false),
      ("inside", false),
      ("vtx_face", false),
      ("vtx_face_gap", false),
      ("vtx_face_over", true),
      ("slide", true),
      ("empty", false),
    ],
  );
}

#[test]
fn intersects_ray_semantics() {
  bools(
    r#"
b = box(1)
hit = intersects_ray(v3(0, 0, -5), v3(0, 0, 1), b)
miss = intersects_ray(v3(0, 5, -5), v3(0, 0, 1), b)
on_out = intersects_ray(v3(0, 0, -0.5), v3(0, 0, -1), b)
on_in = intersects_ray(v3(0, 0, -0.5), v3(0, 0, 1), b)
inside = intersects_ray(v3(0, 0, 0), v3(0, 0, 1), b)
edge = intersects_ray(v3(0.5, 0.5, -5), v3(0, 0, 1), b)
min_plane = intersects_ray(v3(-0.5, 0, -5), v3(0, 0, 1), b)
max_plane = intersects_ray(v3(0.5, 0, -5), v3(0, 0, 1), b)
just_off = intersects_ray(v3(0.5001, 0, -5), v3(0, 0, 1), b)
grazing = intersects_ray(v3(-5, 0.5, 0), v3(1, 0, 0), b)
maxd_short = intersects_ray(v3(0, 0, -5), v3(0, 0, 1), b, 4.0)
maxd_exact = intersects_ray(v3(0, 0, -5), v3(0, 0, 1), b, 4.5)
maxd_over = intersects_ray(v3(0, 0, -5), v3(0, 0, 1), b, 4.6)
unnorm = intersects_ray(v3(0, 0, -5), v3(0, 0, 2), b, 2.5)
unnorm_short = intersects_ray(v3(0, 0, -5), v3(0, 0, 2), b, 2.2)
behind = intersects_ray(v3(0, 0, 5), v3(0, 0, 1), b)
rotated = intersects_ray(v3(0, 0, -5), v3(0, 0, 1), b | rot(0.3, 0.2, 0.1) | trans(0.2, 0.1, 0))
empty = intersects_ray(v3(0, 0, -5), v3(0, 0, 1), mesh())
"#,
    &[
      ("hit", true),
      ("miss", false),
      ("on_out", false),
      ("on_in", true),
      ("inside", true),
      ("edge", true),
      ("min_plane", true),
      ("max_plane", true),
      ("just_off", false),
      ("grazing", true),
      ("maxd_short", false),
      ("maxd_exact", false),
      ("maxd_over", true),
      ("unnorm", true),
      ("unnorm_short", false),
      ("behind", false),
      ("rotated", true),
      ("empty", false),
    ],
  );
}

#[test]
fn self_intersection_and_aabb() {
  let ctx = parse_and_eval_program(
    r#"
clean = is_self_intersecting(box(1))
crossed = is_self_intersecting(box(1) + (box(1) | rot(0.5, 0.3, 0.2)))
crossed_type = crossed.type
none = is_self_intersecting(mesh())
bb = aabb(box(1, 2, 3) | trans(1, 0, 0))
joined_bb = aabb((box(1) | trans(-2, 0, 0)) + (box(1) | trans(2, 0, 0)))
"#,
  )
  .unwrap();
  assert!(matches!(ctx.get_global("clean").unwrap(), Value::Nil));
  assert!(matches!(ctx.get_global("none").unwrap(), Value::Nil));
  assert!(matches!(ctx.get_global("crossed_type").unwrap(), Value::String(s) if s == "segment"));
  let seq = |name: &str| -> Vec<Vec3> {
    let v = ctx.get_global(name).unwrap();
    v.as_sequence()
      .unwrap()
      .consume(&ctx)
      .map(|v| *v.unwrap().as_vec3().unwrap())
      .collect()
  };
  assert_eq!(
    seq("bb"),
    [Vec3::new(0.5, -1., -1.5), Vec3::new(1.5, 1., 1.5)]
  );
  let jb = seq("joined_bb");
  assert_eq!((jb[0].x, jb[1].x), (-2.5, 2.5));
}
