use mesh::{attrs, LinkedMesh};

use crate::parse_and_eval_program;

fn all_have(m: &LinkedMesh<()>, name: &str) -> bool {
  let ch = &m.vertex_channels[name];
  m.vertices.keys().all(|k| ch.get(k).is_some())
}

#[test]
fn extrude_carries_channels() {
  let ctx =
    parse_and_eval_program(r#"m = box(1) | compute_uvs(type="planar") | extrude(v3(0, 0.5, 0))"#)
      .unwrap();
  let v = ctx.get_global("m").unwrap();
  let m = v.as_mesh().unwrap();
  assert!(all_have(&m.mesh, attrs::UV) && all_have(&m.mesh, attrs::TANGENT));
}

#[test]
fn plus_and_join_union_channels() {
  let ctx = parse_and_eval_program(
    r#"
a = box(1) | compute_uvs(type="planar")
b = box(1) | trans(3, 0, 0)
c = a + b
d = join([a, b])
e = b + a
"#,
  )
  .unwrap();
  for name in ["c", "d", "e"] {
    let v = ctx.get_global(name).unwrap();
    let m = v.as_mesh().unwrap();
    assert_eq!(m.mesh.vertices.len(), 16, "{name}");
    assert!(all_have(&m.mesh, attrs::UV), "{name}");
    assert!(all_have(&m.mesh, attrs::TANGENT), "{name}");
  }
}

fn seq_vals(ctx: &crate::EvalCtx, name: &str) -> Vec<crate::Value> {
  let v = ctx.get_global(name).unwrap();
  let seq = v.as_sequence().unwrap();
  seq.consume(ctx).collect::<Result<_, _>>().unwrap()
}

fn err_of(src: &str) -> String {
  format!("{:?}", parse_and_eval_program(src).unwrap_err())
}

#[test]
fn set_attr_bag_warp_and_readback() {
  let ctx = parse_and_eval_program(
    r#"
m = box(1)
  | set_attr("h", |pos| pos.y)
  | set_attr("color", |pos, n, {h, ix}| v3(h, ix, 0))
w = m | warp(|p, n, {color}| p + v3(0, color.x, 0))
w2 = m -> |p, n, {h: hh}| p + v3(0, hh, 0)
orig = m | verts
warped = w | verts
warped2 = w2 | verts
hs = m | attr("h")
names = m | attrs
d = m | drop_attr("h") | attrs
"#,
  )
  .unwrap();
  let mv = ctx.get_global("m").unwrap();
  let m = mv.as_mesh().unwrap();
  let color = &m.mesh.vertex_channels["color"];
  assert!(matches!(
    color.store,
    mesh::linked_mesh::ChannelStore::Vec3(_)
  ));
  for (ix, k) in m.mesh.vertices.keys().enumerate() {
    let c = color.get(k).unwrap();
    assert_eq!(c[0], m.mesh.vertices[k].position.y);
    assert_eq!(c[1], ix as f32);
  }
  assert!(all_have(&m.mesh, "h"));

  let orig = seq_vals(&ctx, "orig");
  for name in ["warped", "warped2"] {
    let warped = seq_vals(&ctx, name);
    assert_eq!(orig.len(), warped.len());
    for (o, w) in orig.iter().zip(&warped) {
      let (o, w) = (o.as_vec3().unwrap(), w.as_vec3().unwrap());
      assert_eq!(w.y, 2. * o.y, "{name}");
      assert_eq!((w.x, w.z), (o.x, o.z), "{name}");
    }
  }

  let hs = seq_vals(&ctx, "hs");
  assert_eq!(hs.len(), 8);
  assert!(hs.iter().all(|v| v.as_float().unwrap().abs() == 0.5));
  let names: Vec<String> = seq_vals(&ctx, "names")
    .iter()
    .map(|v| v.as_str().unwrap().to_owned())
    .collect();
  assert_eq!(names, ["color", "h"]);
  let d: Vec<String> = seq_vals(&ctx, "d")
    .iter()
    .map(|v| v.as_str().unwrap().to_owned())
    .collect();
  assert_eq!(d, ["color"]);
}

#[test]
fn set_attr_seq_form_and_extrude_bag() {
  let ctx = parse_and_eval_program(
    r#"
m = box(1) | set_attr("w", map(|i| i * 0.5, 0..8))
ws = m | attr("w")
e = m | extrude(|p, {w}| v3(0, w, 0))
"#,
  )
  .unwrap();
  let ws = seq_vals(&ctx, "ws");
  assert_eq!(
    ws.iter().map(|v| v.as_float().unwrap()).collect::<Vec<_>>(),
    (0..8).map(|i| i as f32 * 0.5).collect::<Vec<_>>()
  );
  let ev = ctx.get_global("e").unwrap();
  let e = ev.as_mesh().unwrap();
  assert_eq!(e.mesh.vertices.len(), 16);
  assert!(all_have(&e.mesh, "w"));
}

#[test]
fn bag_and_attr_errors() {
  let cases = [
    ("box(1) | warp(|p, n, bag| p)", "must be destructured"),
    (
      "box(1) | warp(|p, n, {nope}| p)",
      "Unknown attribute bag key `nope`",
    ),
    (
      "box(1) | warp(|a, b, c, d| a)",
      "Too many callback parameters",
    ),
    ("box(1) | set_attr(\"ix\", |p| 1)", "reserved"),
    ("box(1) | set_attr(\"uv\", |p| p)", "must be vec2"),
    (
      "box(1) | set_attr(\"x\", [1, 2])",
      "2 values but the mesh has 8 vertices",
    ),
    (
      "box(1) | set_attr(\"bad-name\", |p| 1)",
      "Invalid attribute name",
    ),
    ("box(1) | attr(\"nope\")", "no attribute `nope`"),
    ("box(1) | drop_attr(\"nope\")", "no attribute `nope`"),
    (
      "box(1) | bake_ao(into=\"color\")",
      "writes a scalar attribute",
    ),
  ];
  for (src, needle) in cases {
    let err = err_of(src);
    assert!(err.contains(needle), "{src}\n{err}");
  }
}

#[test]
fn transfer_attrs_is_exact_on_surface() {
  let ctx = parse_and_eval_program(
    r#"
src = box(2) | set_attr("color", |p| p) | set_attr("d", |p| v3(1, 0, 0), spatial="direction")
dst = box(2) | tessellate(0.5) | transfer_attrs(src)
"#,
  )
  .unwrap();
  let v = ctx.get_global("dst").unwrap();
  let m = v.as_mesh().unwrap();
  assert!(m.mesh.vertices.len() > 8);
  let color = &m.mesh.vertex_channels["color"];
  for (k, vtx) in m.mesh.vertices.iter() {
    let c = color.get(k).unwrap();
    let p = vtx.position;
    assert!(
      (c[0] - p.x).abs() < 1e-4 && (c[1] - p.y).abs() < 1e-4 && (c[2] - p.z).abs() < 1e-4,
      "{p:?} vs {c:?}"
    );
  }
  assert_eq!(
    m.mesh.vertex_channels["d"].spatial,
    mesh::linked_mesh::SpatialXform::Direction
  );
}

#[test]
fn faces_index_verts_order() {
  let ctx = parse_and_eval_program("f = box(1) | faces\nn = len(f)\nfirst = f | first").unwrap();
  assert_eq!(ctx.get_global("n").unwrap().as_int().unwrap(), 12);
  let first = seq_vals(&ctx, "first");
  assert_eq!(first.len(), 3);
  assert!(first.iter().all(|v| (0..8).contains(&v.as_int().unwrap())));
}

#[test]
fn smooth_attr_relaxes_pins_and_renormalizes() {
  let ctx = parse_and_eval_program(
    r#"
m = box(2) | tessellate(0.5)
  | set_attr("h", |p, n, {ix}| ix % 2)
  | set_attr("pin", |p| smoothstep(0.9, 1.0, p.y))
  | set_attr("d", |p| normalize(v3(p.x, 1, p.z)), spatial="direction")
s = m | smooth_attr("h", iterations=4, lambda=1, pin="pin")
c = m | smooth_attr("h", iterations=2, weights="cotan")
sd = m | smooth_attr("d", iterations=3)
h0 = m | attr("h")
h1 = s | attr("h")
hc = c | attr("h")
pin = m | attr("pin")
d = sd | attr("d")
"#,
  )
  .unwrap();
  let (h0, h1, hc, pin) = (
    seq_vals(&ctx, "h0"),
    seq_vals(&ctx, "h1"),
    seq_vals(&ctx, "hc"),
    seq_vals(&ctx, "pin"),
  );
  let dev = |vals: &[crate::Value]| {
    vals
      .iter()
      .map(|v| (v.as_float().unwrap() - 0.5).abs())
      .sum::<f32>()
      / vals.len() as f32
  };
  assert!(dev(&h1) < dev(&h0) * 0.5 && dev(&hc) < dev(&h0) * 0.7);
  for ((a, b), p) in h0.iter().zip(&h1).zip(&pin) {
    if p.as_float().unwrap() != 0. {
      assert_eq!(a.as_float().unwrap(), b.as_float().unwrap());
    }
  }
  for v in seq_vals(&ctx, "d") {
    assert!((v.as_vec3().unwrap().norm() - 1.).abs() < 1e-4);
  }
}

#[test]
fn bake_ao_open_vs_enclosed() {
  let ctx = parse_and_eval_program(
    r#"
open = box(2) | bake_ao(samples=16) | attr("ao")
buried = box(1) | bake_ao(samples=16, occluders=[box(4)]) | attr("ao")
local = box(1) | bake_ao(samples=16, occluders=box(4), max_dist=0.5, into="occ") | attr("occ")
"#,
  )
  .unwrap();
  assert!(seq_vals(&ctx, "open")
    .iter()
    .all(|v| v.as_float().unwrap() > 0.98));
  assert!(seq_vals(&ctx, "buried")
    .iter()
    .all(|v| v.as_float().unwrap() < 0.02));
  assert!(seq_vals(&ctx, "local")
    .iter()
    .all(|v| v.as_float().unwrap() > 0.98));
}
