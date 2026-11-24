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

#[test]
fn bake_ao_refine_splits_near_occluder_only() {
  let ctx = parse_and_eval_program(
    r#"
floor = box(40, 0.5, 40)
lid = box(4) | trans(17, 2.25, 0)
flat = floor | bake_ao(samples=32, occluders=lid)
fine = floor | bake_ao(samples=64, occluders=lid, refine=0.06, min_edge=0.5)
"#,
  )
  .unwrap();
  let flat = ctx
    .get_global("flat")
    .unwrap()
    .as_mesh()
    .unwrap()
    .mesh
    .clone();
  let fine = ctx
    .get_global("fine")
    .unwrap()
    .as_mesh()
    .unwrap()
    .mesh
    .clone();
  // seams split by default: six separate faces
  assert_eq!((flat.faces.len(), flat.vertices.len()), (12, 24));
  assert!(fine.faces.len() > 100, "{}", fine.faces.len());
  assert!(all_have(&fine, "ao"));
  let ch = &fine.vertex_channels["ao"];
  let mut near = Vec::new();
  let mut far = Vec::new();
  for f in fine.faces.values() {
    let [a, b, c] = f.vertices.map(|k| fine.vertices[k].position);
    let centroid = (a + b + c) / 3.;
    if centroid.y < 0. {
      continue;
    }
    let longest = [(a - b).norm(), (b - c).norm(), (c - a).norm()]
      .into_iter()
      .fold(0f32, f32::max);
    if ((centroid.x - 17.).powi(2) + centroid.z.powi(2)).sqrt() < 1.5 {
      near.push(longest);
    } else if centroid.x < 0. {
      far.push(longest);
    }
  }
  let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
  assert!(
    mean(&near) * 4. < mean(&far),
    "near {} far {}",
    mean(&near),
    mean(&far)
  );
  for (k, v) in fine.vertices.iter() {
    let ao = ch.get(k).unwrap()[0];
    if v.position.y > 0. && (v.position.x - 17.).abs() < 1.5 && v.position.z.abs() < 1.5 {
      assert!(ao < 0.6, "{ao} at {:?}", v.position);
    } else if v.position.y > 0. && v.position.x < -5. {
      assert!(ao > 0.95, "{ao} at {:?}", v.position);
    }
  }
  // The shadow reaches the +x crease, so its seam edges refined; every boundary edge must still
  // have a bit-identical twin on the other side (no T-junctions).
  let mut twins: fxhash::FxHashMap<[[u32; 3]; 2], usize> = Default::default();
  let mut seam_edges = 0;
  for e in fine.edges.values() {
    if e.faces.len() == 1 {
      seam_edges += 1;
      let [a, b] = e.vertices.map(|k| {
        let p = fine.vertices[k].position;
        [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()]
      });
      *twins
        .entry(if a < b { [a, b] } else { [b, a] })
        .or_default() += 1;
    }
  }
  assert!(seam_edges > 24, "{seam_edges}");
  assert!(twins.values().all(|&n| n == 2), "unmatched seam edges");
}

#[test]
fn degenerate_inputs_stay_finite() {
  let ctx = parse_and_eval_program(
    r#"
flat = box(1) | scale(1, 0, 1)
zero = box(1) | scale(0)
sum = flat + flat
joined = join([zero, zero])
pts = flat | point_distribute(10, world_space=true) | collect
pinched = parametric_surface(u_res=4, v_res=4, adaptive_u_sampling=true, adaptive_v_sampling=true, generator=|u, v| v3(0, 0, 0))
pipe = extrude_pipe(radius=1, path=[v3(0,0,0), v3(0,1,0), v3(0,1,0), v3(0,2,0)])
"#,
  )
  .unwrap();
  for name in ["sum", "joined", "pipe"] {
    let v = ctx.get_global(name).unwrap();
    let m = v.as_mesh().unwrap();
    assert!(!m.mesh.vertices.is_empty(), "{name}");
    let finite = |p: &nalgebra::Vector3<f32>| p.iter().all(|c| c.is_finite());
    assert!(
      m.mesh.vertices.values().all(|v| finite(&v.position)),
      "{name}"
    );
  }
}

#[test]
fn edits_refresh_authored_normals() {
  let ctx = parse_and_eval_program(
    "s = icosphere(1, 3) | compute_normals
     warped = s | warp(|p| p + v3(sin(p.y * 2) * 2, 0, 0))
     shell = s | extrude_along_normals(-0.2)
     slab = grid(1, 4) | compute_normals | extrude(v3(0, -0.5, 0))",
  )
  .unwrap();
  for name in ["warped", "shell", "slab"] {
    let v = ctx.get_global(name).unwrap();
    let m = &v.as_mesh().unwrap().mesh;
    let mut fresh = (**m).clone();
    fresh.shading_normals.clear();
    fresh.compute_vertex_displacement_normals();
    assert_eq!(m.shading_normals.len(), m.vertices.len(), "{name}");
    for (k, n) in m.shading_normals.iter() {
      assert!(
        n.dot(&fresh.displacement_normal(k).unwrap()) > 0.9,
        "{name}"
      );
    }
  }
}

#[test]
fn sdf_sign_distance_and_open_meshes() {
  let ctx = parse_and_eval_program(
    r#"
m = box(2) | trans(10, 0, 0)
inside = sdf(m, vec3(10, 0, 0))
outside = sdf(m, vec3(13, 0, 0))
unsigned = sdf(m, vec3(10, 0, 0), signed=false)
corner = sdf(m, vec3(12, 2, 2))
"#,
  )
  .unwrap();
  let f = |n: &str| ctx.get_global(n).unwrap().as_float().unwrap();
  assert!((f("inside") + 1.).abs() < 1e-5, "{}", f("inside"));
  assert!((f("outside") - 2.).abs() < 1e-5, "{}", f("outside"));
  assert!((f("unsigned") - 1.).abs() < 1e-5);
  assert!((f("corner") - 3f32.sqrt()).abs() < 1e-4, "{}", f("corner"));
}

#[test]
fn set_attr_refine_tracks_mask_edge() {
  let ctx = parse_and_eval_program(
    r#"
floor = box(40, 0.5, 40) | set_attr("uv", |p| vec2(p.x, p.z))
mask = box(6) | trans(10, 0, 0)
field = |p| 1 - smoothstep(0, 1, sdf(mask, p))
flat = floor | set_attr("t", field)
fine = floor | set_attr("t", field, refine=0.05, min_edge=0.25, max_edge=4)
"#,
  )
  .unwrap();
  let mesh_of = |n: &str| ctx.get_global(n).unwrap().as_mesh().unwrap().mesh.clone();
  let (flat, fine) = (mesh_of("flat"), mesh_of("fine"));
  assert_eq!((flat.faces.len(), flat.vertices.len()), (12, 8));
  assert!(fine.faces.len() > 100, "{}", fine.faces.len());
  assert!(all_have(&fine, "t") && all_have(&fine, "uv"));
  let t = &fine.vertex_channels["t"];
  let uv = &fine.vertex_channels["uv"];
  let (mut near, mut far) = (Vec::new(), Vec::new());
  for f in fine.faces.values() {
    let [a, b, c] = f.vertices.map(|k| fine.vertices[k].position);
    let centroid = (a + b + c) / 3.;
    if centroid.y < 0. {
      continue;
    }
    let longest = [(a - b).norm(), (b - c).norm(), (c - a).norm()]
      .into_iter()
      .fold(0f32, f32::max);
    let ring = ((centroid.x - 10.).abs() - 3.).abs() < 1. && centroid.z.abs() < 3.;
    if ring {
      near.push(longest);
    } else if centroid.x < -5. {
      far.push(longest);
    }
  }
  let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
  assert!(
    mean(&near) * 4. < mean(&far),
    "near {} far {}",
    mean(&near),
    mean(&far)
  );
  for (k, v) in fine.vertices.iter() {
    let p = v.position;
    let tv = t.get(k).unwrap()[0];
    if p.y > 0. && (p.x - 10.).abs() < 2.9 && p.z.abs() < 2.9 {
      assert!((tv - 1.).abs() < 1e-4, "{tv} at {p:?}");
    } else if p.x < -5. {
      assert!(tv.abs() < 1e-4, "{tv} at {p:?}");
    }
    let [u, w, ..] = uv.get(k).unwrap();
    assert!(
      (u - p.x).abs() < 1e-4 && (w - p.z).abs() < 1e-4,
      "uv not interpolated at {p:?}"
    );
  }
}

#[test]
fn refine_survives_zero_width_boundary_face() {
  let ctx = parse_and_eval_program(
    r#"
sliver = mesh([v3(0, 0, 0), v3(2, 0, 0), v3(2, 0, 0)], [0, 1, 2])
fine = sliver | set_attr("t", |p| p.x * p.x, refine=0.01, min_edge=0.2, max_edge=0.5)
"#,
  )
  .unwrap();
  let fine = ctx.get_global("fine").unwrap();
  assert!(fine.as_mesh().unwrap().mesh.faces.len() > 4);
}

#[test]
fn set_attr_refine_rejects_vertex_only_inputs() {
  assert!(err_of(r#"box(2) | set_attr("t", |p, n, {ix}| ix, refine=0.1)"#).contains("refine"));
  assert!(err_of(r#"box(2) | set_attr("t", 0..8, refine=0.1)"#).contains("callable"));
}

#[test]
fn bake_texture_and_bake_attr_over_planar_atlas() {
  let ctx = parse_and_eval_program(
    r#"
g = grid(10, 4) | set_attr("uv", |p| vec2(p.x + 5, p.z + 5) * 0.05) | set_attr("w", |p| p.x + 100)
pos = g | bake_texture(64, 64, |p| p + vec3(100, 0, 0), dilate=0)
padded = g | bake_texture(64, 64, |p| p + vec3(100, 0, 0), dilate=6)
by_uv = g | bake_texture(64, 64, |p, uv| uv.x, dilate=0)
w = g | bake_attr(64, 64, "w", dilate=0)
n_pos = texture_mean(smoothstep(0.4, 0.6, pos.r))
n_padded = texture_mean(smoothstep(0.4, 0.6, padded.r))
here = g | bake_texture(64, 64, |p| p, transformed=true, dilate=0)
moved = g | trans(50, 0, 0) | bake_texture(64, 64, |p| p, transformed=true, dilate=0)
shift = texture_mean(moved.r) - texture_mean(here.r)
"#,
  )
  .unwrap();
  let tex = |n: &str| ctx.get_global(n).unwrap().as_texture().unwrap().clone();
  let (pos, w, by_uv) = (tex("pos"), tex("w"), tex("by_uv"));
  assert_eq!((pos.width, pos.height, pos.channels), (64, 64, 3));
  assert_eq!((w.channels, by_uv.channels), (1, 1));
  let (pp, wp, up) = (pos.as_planes(), w.as_planes(), by_uv.as_planes());
  let mut covered = 0;
  for i in 0..64 * 64 {
    if pp[0][i] == 0. {
      assert_eq!((pp[1][i], pp[2][i], wp[0][i]), (0., 0., 0.));
      continue;
    }
    covered += 1;
    assert!(
      (95. ..=105.).contains(&pp[0][i]) && pp[1][i].abs() < 1e-4,
      "{:?}",
      (pp[0][i], pp[1][i])
    );
    assert!(
      (wp[0][i] - pp[0][i]).abs() < 1e-3,
      "attr {} vs pos {}",
      wp[0][i],
      pp[0][i]
    );
    assert!((up[0][i] - (i % 64) as f32 / 64. - 0.5 / 64.).abs() < 1e-5);
  }
  assert!(
    (covered as f32 / (64. * 64.) - 0.25).abs() < 0.03,
    "{covered}"
  );
  let f = |n: &str| ctx.get_global(n).unwrap().as_float().unwrap();
  assert!((f("shift") - 12.5).abs() < 1.5, "{}", f("shift"));
  assert!(
    f("n_padded") > f("n_pos") + 0.02,
    "{} vs {}",
    f("n_padded"),
    f("n_pos")
  );
  assert!(err_of(r#"box(2) | bake_texture(8, 8, |p| p)"#).contains("compute_uvs"));
  assert!(
    err_of(r#"box(2) | compute_uvs(type="planar") | bake_texture(8, 8, |p, n, {ix}| p)"#)
      .contains("bake_attr")
  );
}

#[test]
fn split_by_field_cuts_on_the_contour() {
  let ctx = parse_and_eval_program(
    r#"
floor = grid(20, 10)
mask = cylinder(4, 4, 64)
parts = floor | split_by_field(mask, refine=0.02, min_edge=0.1)
inside = parts[0]
outside = parts[1]
lin = floor | split_by_field(|p| p.x - 3.)
lin_in = lin[0]
lin_out = lin[1]
"#,
  )
  .unwrap();
  let mesh_of = |n: &str| ctx.get_global(n).unwrap().as_mesh().unwrap().mesh.clone();
  let (inside, outside) = (mesh_of("inside"), mesh_of("outside"));
  let area = |m: &LinkedMesh<()>| {
    m.faces
      .values()
      .map(|f| {
        let [a, b, c] = f.vertices.map(|k| m.vertices[k].position);
        (b - a).cross(&(c - a)).norm() * 0.5
      })
      .sum::<f32>()
  };
  assert!(inside.faces.len() > 20 && outside.faces.len() > 20);
  assert!((area(&inside) + area(&outside) - 400.).abs() < 0.5);
  assert!(
    (area(&inside) - std::f32::consts::PI * 16.).abs() < 2.,
    "{}",
    area(&inside)
  );
  for v in inside.vertices.values() {
    assert!(v.position.xz().norm() < 4.05, "{:?}", v.position);
  }
  for v in outside.vertices.values() {
    assert!(v.position.xz().norm() > 3.95, "{:?}", v.position);
  }
  assert!(inside.vertices.len() + outside.vertices.len() < 1500);
  let (li, lo) = (mesh_of("lin_in"), mesh_of("lin_out"));
  assert!(li.vertices.values().all(|v| v.position.x <= 3. + 1e-4));
  assert!(lo.vertices.values().all(|v| v.position.x >= 3. - 1e-4));
  assert!(
    li.vertices
      .values()
      .filter(|v| (v.position.x - 3.).abs() < 1e-4)
      .count()
      >= 11
  );
}

#[test]
fn bake_sdf_triplanar_and_decal() {
  let ctx = parse_and_eval_program(
    r#"
g = grid(10, 2) | set_attr("uv", |p| vec2(p.x + 5, p.z + 5) * 0.1)
mask = box(2) | trans(2, 0, 1)
d = g | bake_sdf(64, 64, mask, max_dist=3)
ref = g | bake_texture(64, 64, |p| clamp(-3, 3, sdf(mask, p)))
err = texture_max(abs(d - ref))
stamp = texture(8, 8, |uv| vec3(1., 0.5, 0.25))
dec = g | bake_decal(64, 64, stamp, transform=mat4() | trans(2, 0, -1) | rot(-pi / 2, 0, 0) | scale(4))
cover = texture_mean(dec.a)
tri = g | bake_triplanar(32, 32, texture(16, 16, |uv| 0.75), scale=0.5)
tri_mean = texture_mean(tri)
"#,
  )
  .unwrap();
  let f = |n: &str| ctx.get_global(n).unwrap().as_float().unwrap();
  assert!(f("err") < 1e-3, "{}", f("err"));
  assert!((f("cover") - 0.16).abs() < 0.02, "{}", f("cover"));
  assert!((f("tri_mean") - 0.75).abs() < 0.01, "{}", f("tri_mean"));
  let dec = ctx.get_global("dec").unwrap().as_texture().unwrap().clone();
  assert_eq!(dec.channels, 4);
  let (px, a) = (dec.as_planes(), dec.as_planes()[3].clone());
  let i = 25 * 64 + 44; // world (2, -1) -> uv (0.7, 0.4): inside the stamp
  assert!(
    a[i] == 1. && (px[0][i] - 1.).abs() < 1e-4 && (px[1][i] - 0.5).abs() < 1e-4,
    "{:?}",
    (a[i], px[0][i])
  );
  let j = 5 * 64 + 5; // far corner: untouched
  assert_eq!((a[j], px[0][j]), (0., 0.));
}

#[test]
fn bake_ao_texture_resolves_sub_triangle_occlusion() {
  let ctx = parse_and_eval_program(
    r#"
floor = grid(40, 1) | set_attr("uv", |p| vec2(p.x + 20, p.z + 20) / 40)
lid = box(4, 0.5, 4) | trans(10, 1, 0)
ao = floor | bake_ao_texture(64, 64, samples=32, occluders=lid, max_dist=4)
under = texture_mean(crop(46, 30, 4, 4, ao))
open = texture_mean(crop(4, 4, 8, 8, ao))
deep = floor | bake_ao_texture(64, 64, samples=32, occluders=lid, max_dist=4, intensity=2)
deep_under = texture_mean(crop(46, 30, 4, 4, deep))
deep_open = texture_mean(crop(4, 4, 8, 8, deep))
v = floor | bake_ao(occluders=lid, max_dist=4) | bake_ao(occluders=lid, max_dist=4, into="ao2", intensity=2)
"#,
  )
  .unwrap();
  let f = |n: &str| ctx.get_global(n).unwrap().as_float().unwrap();
  let ao = ctx.get_global("ao").unwrap().as_texture().unwrap().clone();
  assert_eq!((ao.width, ao.height, ao.channels), (64, 64, 1));
  assert!(f("under") < 0.5, "{}", f("under"));
  assert_eq!(
    f("open"),
    1.,
    "open texels cast no rays and must be exactly 1"
  );
  assert!(
    f("deep_under") < f("under") * 0.75,
    "{} vs {}",
    f("deep_under"),
    f("under")
  );
  assert_eq!(f("deep_open"), 1.);
  let v = ctx.get_global("v").unwrap();
  let m = &v.as_mesh().unwrap().mesh;
  let (ao, ao2) = (&m.vertex_channels["ao"], &m.vertex_channels["ao2"]);
  for k in m.vertices.keys() {
    let (a, b) = (ao.get(k).unwrap()[0], ao2.get(k).unwrap()[0]);
    assert!((a * a - b).abs() < 1e-5, "{a}² vs {b}");
  }
}

#[test]
fn blur_atlas_stays_inside_islands() {
  // Two islands side by side in the atlas with a gutter; a blur wider than the gutter must not
  // mix them, because they are 20 units apart on the surface.
  let ctx = parse_and_eval_program(
    r#"
a = grid(4, 1) | set_attr("uv", |p| vec2(p.x + 2, p.z + 2) / 4 * vec2(0.4, 1))
b = grid(4, 1) | trans(20, 0, 0) | set_attr("uv", |p| vec2(p.x + 2, p.z + 2) / 4 * vec2(0.4, 1) + vec2(0.6, 0))
m = a + b
t = m | bake_texture(64, 64, |p| if p.x > 10 { 1. } else { 0. }, dilate=0)
s = t | blur_atlas(6, m, dilate=0)
left = texture_mean(crop(2, 8, 20, 48, s))
right = texture_mean(crop(42, 8, 20, 48, s))
"#,
  )
  .unwrap();
  let f = |n: &str| ctx.get_global(n).unwrap().as_float().unwrap();
  assert!(
    f("left") < 1e-4 && (f("right") - 1.).abs() < 1e-4,
    "{} {}",
    f("left"),
    f("right")
  );
}

#[test]
fn texture_memory_limit_evicts_then_reports_by_line() {
  use crate::tex_mem;
  let src = r#"
g = grid(10, 4) | set_attr("uv", |p| vec2(p.x + 5, p.z + 5) * 0.1)
a = g | bake_texture(256, 256, |p| p)
b = g | bake_texture(128, 128, |p| p.x)
c = a * 2
d = c + a
"#;
  // Unlimited: the atlas cache keeps both rasters alive.
  let ctx = parse_and_eval_program(src).unwrap();
  let plane = 256 * 256 * 4;
  assert_eq!(ctx.uv_rasters.borrow().len(), 2);
  drop(ctx);
  assert_eq!(tex_mem::live_bytes(), 0);

  // Room for a, b, c and d only once the atlases are evicted.
  tex_mem::set_limit(Some(9 * plane + plane / 4 + plane / 2));
  let ctx = parse_and_eval_program(src).unwrap();
  assert!(ctx.uv_rasters.borrow().is_empty());
  drop(ctx);

  tex_mem::set_limit(Some(7 * plane));
  let err = format!("{}", parse_and_eval_program(src).err().unwrap());
  tex_mem::set_limit(None);
  assert!(err.starts_with("at line 6"), "{err}");
  assert!(
    err.contains("Out of memory: could not allocate 0.2 MB at line 6"),
    "{err}"
  );
  assert!(err.contains("of a 1.8 MB limit (after evicting"), "{err}");
  assert!(
    err.contains("texture plane ×3 at line 3") && err.contains("texture plane ×3 at line 5"),
    "{err}"
  );
}

fn volume(m: &LinkedMesh<()>) -> f32 {
  m.faces
    .values()
    .map(|f| {
      let [a, b, c] = f.vertices.map(|k| m.vertices[k].position);
      a.dot(&b.cross(&c)) / 6.
    })
    .sum()
}

#[test]
fn displace_offsets_refines_and_stays_closed() {
  let ctx = parse_and_eval_program(
    r#"
inflated = box(2) | displace(0.25)
seamed = cylinder(1, 2, 16) | compute_uvs(type="cylinder") | displace(|p| 0.1 + 0.05 * sin(p.x * 3))
bump = |p| 0.5 * exp(-(p.x * p.x + p.z * p.z) * 4)
coarse = grid(4, 1) | displace(bump)
fine = grid(4, 1) | displace(bump, refine=0.005, min_edge=0.02)
bent = grid(4, 1) | warp(|p| v3(p.x, sin(p.x * 2), p.z), refine=0.01, min_edge=0.02)
ramp = grid(4, 1) | displace(|p| 0.5 * p.x, split_seams=true)
welded = box(2) | displace(|p| 0.1 * abs(p.x), refine=0.01, min_edge=0.1)
"#,
  )
  .unwrap();
  let mesh_of = |n: &str| ctx.get_global(n).unwrap().as_mesh().unwrap().mesh.clone();

  // Mitered: every face moves out by exactly the height, corners stay sharp.
  let inflated = mesh_of("inflated");
  assert_eq!(inflated.vertices.len(), 8);
  for v in inflated.vertices.values() {
    assert!(
      v.position.iter().all(|c| (c.abs() - 1.25).abs() < 1e-4),
      "{:?}",
      v.position
    );
  }

  // Every vertex of the unwrapped cylinder has a seam twin, and still does.
  let seamed = mesh_of("seamed");
  assert_eq!(seamed.vertices.len(), 68);
  assert_eq!(
    crate::mesh_ops::displace::coincident_groups(&seamed).len(),
    34
  );

  // Without `split_seams` a box stays a closed 2-manifold, its creases only marked.
  let welded = mesh_of("welded");
  welded.check_is_manifold::<true>().unwrap();
  assert!(welded.shading_normals.is_empty());
  assert!(welded.edges.values().any(|e| e.sharp));

  // Authored normals follow the field's slope, not the (here 2-triangle) mesh.
  let ramp = mesh_of("ramp");
  assert_eq!(ramp.shading_normals.len(), ramp.vertices.len());
  let tilt = mesh::linked_mesh::Vec3::new(-0.5, 1., 0.).normalize();
  for n in ramp.shading_normals.values() {
    assert!((n - tilt).norm() < 1e-3, "{n:?}");
  }

  let peak = |m: &LinkedMesh<()>| m.vertices.values().map(|v| v.position.y).fold(0., f32::max);
  let (coarse, fine) = (mesh_of("coarse"), mesh_of("fine"));
  assert!(fine.vertices.len() > 20 * coarse.vertices.len());
  assert!((peak(&fine) - 0.5).abs() < 0.01, "{}", peak(&fine));
  for v in fine.vertices.values() {
    let p = v.position;
    assert!((p.y - 0.5 * (-(p.x * p.x + p.z * p.z) * 4.).exp()).abs() < 1e-4);
  }

  let bent = mesh_of("bent");
  assert!(bent.vertices.len() > 40);
  for v in bent.vertices.values() {
    assert!((v.position.y - (v.position.x * 2.).sin()).abs() < 1e-4);
  }
}

#[test]
fn emboss_raises_a_region_and_stays_manifold() {
  let ctx = parse_and_eval_program(
    r#"
slab = box(4, 1, 4)
raised = slab | emboss(|p| max(abs(p.x), abs(p.z)) - 1, 0.5, refine=0.01, min_edge=0.02, max_edge=1)
sunk = slab | emboss(cylinder(1, 4, 32), -0.25, refine=0.01, min_edge=0.05)
// wraps the box's vertical edges and touches itself at corners of the checker
checker = slab | emboss(|p| sin(p.x * 3) * sin(p.z * 3) * sin(p.y * 3 + 1), 0.1, max_edge=0.3)
ramp = slab | emboss(|p| p.x, |p| max(0, -p.x) * 0.2)
tagged = slab | emboss(cylinder(1, 4, 32), 0.25, tag="top") | split_by_field(|p, n, {top}| 0.5 - top)
top = tagged[0]
"#,
  )
  .unwrap();
  let mesh_of = |n: &str| ctx.get_global(n).unwrap().as_mesh().unwrap().mesh.clone();
  for name in ["raised", "sunk", "checker", "ramp"] {
    mesh_of(name)
      .check_is_manifold::<true>()
      .unwrap_or_else(|err| panic!("{name}: {err:?}"));
  }
  // The 2x2 square is raised on the top face and the matching one on the bottom.
  let raised = volume(&mesh_of("raised"));
  assert!((raised - (16. + 2. * 4. * 0.5)).abs() < 0.02, "{raised}");
  // Both discs plus the upper half of their walls.
  let top = mesh_of("top");
  assert!(top.vertices.values().all(|v| v.position.y.abs() > 0.6));
  let sunk = volume(&mesh_of("sunk"));
  assert!(
    (sunk - (16. - 2. * std::f32::consts::PI * 0.25)).abs() < 0.02,
    "{sunk}"
  );
}

#[test]
fn emboss_stamps_a_path_through_a_surface_chart() {
  let ctx = parse_and_eval_program(
    r#"
sheet = grid(4, 4) | surface_chart(v3(0), up=v3(0, 0, -1))
stamped = sheet | emboss(rect(v2(0), v2(2, 1)), 0.3, refine=0.01, min_edge=0.02)
domed = sheet | emboss(rect(v2(0), v2(2, 1)), |p, n, d| -d, refine=0.01, min_edge=0.02)
"#,
  )
  .unwrap();
  let mesh_of = |n: &str| ctx.get_global(n).unwrap().as_mesh().unwrap().mesh.clone();

  // On a flat sheet the chart is the plane itself, so the raised top is the rectangle.
  let stamped = mesh_of("stamped");
  let top = || {
    stamped
      .vertices
      .values()
      .map(|v| v.position)
      .filter(|p| p.y > 0.29)
  };
  let reach = |axis: usize| top().map(|p| p[axis].abs()).fold(0., f32::max);
  assert!(
    (reach(0) - 1.).abs() < 0.03 && (reach(2) - 0.5).abs() < 0.03,
    "{} {}",
    reach(0),
    reach(2)
  );
  assert!(top().count() > 8);

  // `dist` is the depth in from the outline: the rectangle's centre line is 0.5 from its edge.
  let peak = mesh_of("domed")
    .vertices
    .values()
    .map(|v| v.position.y)
    .fold(0., f32::max);
  assert!((peak - 0.5).abs() < 0.03, "{peak}");
}

#[test]
fn height_creases_land_on_mesh_edges() {
  let ctx = parse_and_eval_program(
    r#"
roof = grid(4, 3) | displace(|p| 0.5 - abs(p.x - 0.3), refine=0.01, min_edge=0.05)
ring = grid(4, 8) | displace(|p| max(0, 0.3 - abs(len(v2(p.x, p.z)) - 1.1)), refine=0.002, min_edge=0.005)
// two holes 0.28 apart: their chamfers meet in a ridge at x = 0.47, above the floor
holes = trace_svg_path("M-1,-1 L2,-1 L2,1 L-1,1 Z M0.33,0 A0.26,0.26 0 1 0 -0.19,0 A0.26,0.26 0 1 0 0.33,0 Z M1.13,0 A0.26,0.26 0 1 0 0.61,0 A0.26,0.26 0 1 0 1.13,0 Z")
seamed = box(2) | displace(|p| 0.1 * abs(p.x - 0.3) + 0.07 * abs(p.y - 0.13) + 0.05 * abs(p.z + 0.41), refine=0.01, min_edge=0.1, split_seams=true)
// a crease 0.03 from the box's edge
edge = box(2) | displace(|p, n, {pos}| { q = abs(pos - v3(0.97, 0, 0)); 0.5 * q.x }, refine=0.01, min_edge=0.02)
plate = grid(4, 4) | surface_chart(v3(0), up=v3(0, 0, -1))
  | emboss(holes, |p, n, d| max(d * 0.8, -0.12), refine=0.002, min_edge=0.005)
"#,
  )
  .unwrap();
  let roof = ctx
    .get_global("roof")
    .unwrap()
    .as_mesh()
    .unwrap()
    .mesh
    .clone();
  // The ridge at x = 0.3 is on no grid line; every vertex beside it would otherwise sit below.
  let ridge = roof
    .vertices
    .values()
    .filter(|v| (v.position.x - 0.3).abs() < 1e-4);
  assert!(ridge.clone().count() >= 6);
  assert!(ridge.clone().all(|v| (v.position.y - 0.5).abs() < 1e-3));
  let (lo, hi) = ridge.fold((f32::MAX, f32::MIN), |(lo, hi), v| {
    (lo.min(v.position.z), hi.max(v.position.z))
  });
  assert!(hi - lo > 3.9, "{lo} {hi}");

  // A curved crease is followed by vertices on it, not bisected toward down to `min_edge`.
  let n_verts = |n: &str| {
    ctx
      .get_global(n)
      .unwrap()
      .as_mesh()
      .unwrap()
      .mesh
      .vertices
      .len()
  };
  assert!(n_verts("ring") < 4000, "{}", n_verts("ring"));

  // Both sides of a seam split at the same points: every edge has its coincident twin.
  let seamed = ctx
    .get_global("seamed")
    .unwrap()
    .as_mesh()
    .unwrap()
    .mesh
    .clone();
  let mut uses: fxhash::FxHashMap<[[u32; 3]; 2], u32> = Default::default();
  for f in seamed.faces.values() {
    for i in 0..3 {
      let [a, b]: [[u32; 3]; 2] = [i, (i + 1) % 3].map(|i| {
        seamed.vertices[f.vertices[i]]
          .position
          .map(f32::to_bits)
          .into()
      });
      *uses.entry([a.min(b), a.max(b)]).or_default() += 1;
    }
  }
  assert!(uses.values().all(|&n| n == 2));

  // A bag param and a vector-valued `abs` still give guards; no corner slides off the box's
  // edge to reach the crease.
  let edge = ctx
    .get_global("edge")
    .unwrap()
    .as_mesh()
    .unwrap()
    .mesh
    .clone();
  let max_x = edge
    .vertices
    .values()
    .map(|v| v.position.x)
    .fold(f32::MIN, f32::max);
  assert!(
    edge.vertices.len() < 100 && (max_x - 1.015).abs() < 1e-4,
    "{} {max_x}",
    edge.vertices.len()
  );

  let plate = ctx
    .get_global("plate")
    .unwrap()
    .as_mesh()
    .unwrap()
    .mesh
    .clone();
  let on_ridge = plate.vertices.values().filter(|v| {
    let p = v.position;
    (p.x - 0.47).abs() < 1e-4 && p.z.abs() < 0.08 && (p.y + 0.112).abs() < 2e-3
  });
  assert!(on_ridge.count() >= 3);
}
