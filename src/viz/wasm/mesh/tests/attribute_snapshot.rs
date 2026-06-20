//! Behavioral anchor for the vertex/edge attribute-channel refactor.
//!
//! Exercises the public mesh pipeline that propagates per-vertex attributes —
//! sharpness marking, vertex separation + normal computation, and indexed export
//! — and pins the exported buffers so an internal storage refactor can't silently
//! change observable output. Uses only the public API on purpose.
//!
//! Regenerate the fixture after an intentional change with:
//!   BLESS=1 cargo test -p mesh --test attribute_snapshot

use std::fmt::Write as _;

use mesh::linked_mesh::{
  Arity, Channel, ChannelStore, FlipXform, Interp, LinkedMesh, Plane, Vec3, VertexKey,
};
use mesh::OwnedIndexedMesh;

const SNAPSHOT_PATH: &str =
  concat!(env!("CARGO_MANIFEST_DIR"), "/tests/snapshots/attribute_export.snap");

/// Generous enough to absorb cross-platform float noise (last-ULP normalize/trig
/// differences) while any real attribute regression is orders of magnitude larger.
const TOL: f32 = 1e-4;

fn build_snapshot() -> String {
  let mut out = String::new();

  // All-sharp box: every corner splits into 3 smooth fans, each with its own
  // face normal. Heaviest exercise of the seam-clone + per-face normal path.
  {
    let mut m: LinkedMesh<()> = LinkedMesh::new_box(2., 2., 2.);
    m.mark_edge_sharpness(0.8);
    m.separate_vertices_and_compute_normals();
    serialize_case("box_sharp", &m.to_raw_indexed(true, true, false), &mut out);
  }

  // Box cut by a diagonal plane before normal computation: exercises the
  // edge/face-split vertex creation paths feeding into separation + export.
  {
    let mut m: LinkedMesh<()> = LinkedMesh::new_box(2., 2., 2.);
    m.subdivide_by_plane(&Plane {
      normal: Vec3::new(1., 1., 0.).normalize(),
      w: 0.,
    });
    m.mark_edge_sharpness(0.8);
    m.separate_vertices_and_compute_normals();
    serialize_case("box_subdivided", &m.to_raw_indexed(true, true, false), &mut out);
  }

  // Closed smooth surface: dihedral angles stay below threshold, so vertices are
  // not split and normals are area/angle-averaged across full fans.
  {
    let mut m: LinkedMesh<()> = LinkedMesh::new_icosphere(1., 1);
    m.mark_edge_sharpness(0.8);
    m.separate_vertices_and_compute_normals();
    serialize_case("icosphere_smooth", &m.to_raw_indexed(true, true, false), &mut out);
  }

  // Same sharp box, then normals flipped — covers the flip negate/rewind path.
  {
    let mut m: LinkedMesh<()> = LinkedMesh::new_box(2., 2., 2.);
    m.mark_edge_sharpness(0.8);
    m.separate_vertices_and_compute_normals();
    m.flip_normals();
    serialize_case("box_flipped", &m.to_raw_indexed(true, true, false), &mut out);
  }

  // Displacement-normal-only export path (the other real export config), on a
  // mesh with hemispherical caps + pole vertices.
  {
    let mut m: LinkedMesh<()> = LinkedMesh::new_capsule(1., 2., 4, 8, 1);
    m.compute_vertex_displacement_normals();
    serialize_case(
      "capsule_displacement",
      &m.to_raw_indexed(false, false, true),
      &mut out,
    );
  }

  out
}

fn serialize_case(name: &str, m: &OwnedIndexedMesh, out: &mut String) {
  writeln!(out, "## {name}").unwrap();
  writeln!(out, "verts {}", m.vertices.len() / 3).unwrap();
  writeln!(out, "tris {}", m.indices.len() / 3).unwrap();
  write_floats("pos", Some(&m.vertices), out);
  write_floats("shading", m.shading_normals.as_deref(), out);
  write_floats("displacement", m.displacement_normals.as_deref(), out);
  write!(out, "idx").unwrap();
  for i in &m.indices {
    write!(out, " {i}").unwrap();
  }
  writeln!(out).unwrap();
}

fn write_floats(label: &str, vals: Option<&[f32]>, out: &mut String) {
  match vals {
    None => writeln!(out, "{label} none").unwrap(),
    Some(vals) => {
      write!(out, "{label}").unwrap();
      for &v in vals {
        // Canonicalize -0.0 and sub-tolerance noise so the committed fixture is stable.
        let v = if v.abs() < 1e-6 { 0.0 } else { v };
        write!(out, " {v:.6}").unwrap();
      }
      writeln!(out).unwrap();
    }
  }
}

#[test]
fn attribute_export_snapshot() {
  let actual = build_snapshot();

  if std::env::var("BLESS").is_ok() {
    let path = std::path::Path::new(SNAPSHOT_PATH);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, &actual).unwrap();
    eprintln!("blessed snapshot -> {SNAPSHOT_PATH}");
    return;
  }

  let expected = std::fs::read_to_string(SNAPSHOT_PATH).unwrap_or_else(|e| {
    panic!("missing snapshot {SNAPSHOT_PATH}: {e}\nrun `BLESS=1 cargo test -p mesh --test attribute_snapshot` to generate it");
  });

  compare(&expected, &actual);
}

/// Line-aligned compare: float channels match within `TOL`; everything else
/// (counts, indices, headers, presence) must match exactly.
fn compare(expected: &str, actual: &str) {
  let el: Vec<&str> = expected.lines().collect();
  let al: Vec<&str> = actual.lines().collect();
  assert_eq!(
    el.len(),
    al.len(),
    "snapshot line count differs (expected {}, actual {}) — likely a structural change",
    el.len(),
    al.len()
  );

  for (i, (e, a)) in el.iter().zip(&al).enumerate() {
    let (e_label, e_rest) = e.split_once(' ').unwrap_or((e, ""));
    let (a_label, a_rest) = a.split_once(' ').unwrap_or((a, ""));
    assert_eq!(e_label, a_label, "line {i}: label mismatch\n  expected: {e}\n  actual:   {a}");

    let is_floats = matches!(e_label, "pos" | "shading" | "displacement") && e_rest != "none";
    if is_floats {
      let ev = parse_floats(e_rest);
      let av = parse_floats(a_rest);
      assert_eq!(
        ev.len(),
        av.len(),
        "line {i} ({e_label}): float count differs (expected {}, actual {})",
        ev.len(),
        av.len()
      );
      for (j, (x, y)) in ev.iter().zip(&av).enumerate() {
        assert!(
          (x - y).abs() <= TOL,
          "line {i} ({e_label}) element {j}: {x} vs {y} (Δ {} > {TOL})",
          (x - y).abs()
        );
      }
    } else {
      assert_eq!(e, a, "line {i}: mismatch");
    }
  }
}

fn parse_floats(s: &str) -> Vec<f32> {
  s.split_whitespace().map(|t| t.parse().unwrap()).collect()
}

/// Readable spec of the seam-split invariant the refactor must preserve: on an
/// all-sharp box each of the 8 corners duplicates into 3 vertices (one per
/// incident face), and every exported shading normal is an axis direction that
/// appears exactly 4× (once per corner of its face).
#[test]
fn box_corners_split_into_axis_aligned_face_normals() {
  let mut m: LinkedMesh<()> = LinkedMesh::new_box(2., 2., 2.);
  m.mark_edge_sharpness(0.8);
  m.separate_vertices_and_compute_normals();
  let raw = m.to_raw_indexed(true, true, false);

  let shading = raw.shading_normals.as_ref().expect("shading normals present");
  assert_eq!(shading.len() / 3, 24, "expected 8 corners × 3 fans = 24 verts");

  let axes = [Vec3::x(), -Vec3::x(), Vec3::y(), -Vec3::y(), Vec3::z(), -Vec3::z()];
  let mut counts = [0usize; 6];
  for n in shading.chunks_exact(3) {
    let n = Vec3::new(n[0], n[1], n[2]);
    let ax = axes
      .iter()
      .position(|a| (a - n).norm() < TOL)
      .unwrap_or_else(|| panic!("non-axis-aligned box normal {n:?}"));
    counts[ax] += 1;
  }
  assert_eq!(counts, [4; 6], "each face normal should appear 4×");
}

/// `remove_vertex` / `remove_edge` must sweep the attribute channels too, otherwise stale entries
/// accumulate as the mesh is edited. This is invisible to the export snapshot (stale entries are
/// version-checked and never read), so it needs a direct assertion.
#[test]
fn removal_clears_channel_entries() {
  let mut m: LinkedMesh<()> = LinkedMesh::new_box(2., 2., 2.);
  m.compute_edge_displacement_normals();
  m.compute_vertex_displacement_normals();

  let vtx_key = m.vertices.keys().next().unwrap();
  let edge_key = m.edges.keys().next().unwrap();
  assert!(m.displacement_normals.get(vtx_key).is_some());
  assert!(m.edge_displacement_normals.get(edge_key).is_some());
  let (verts_before, edges_before) = (m.displacement_normals.len(), m.edge_displacement_normals.len());

  m.remove_vertex(vtx_key);
  m.remove_edge(edge_key);

  assert!(m.displacement_normals.get(vtx_key).is_none(), "vertex channel entry not cleared");
  assert!(m.edge_displacement_normals.get(edge_key).is_none(), "edge channel entry not cleared");
  assert_eq!(m.displacement_normals.len(), verts_before - 1);
  assert_eq!(m.edge_displacement_normals.len(), edges_before - 1);
}

fn uv(m: &LinkedMesh<()>, k: VertexKey) -> Option<[f32; 2]> {
  match &m.vertex_channels["uv"].store {
    ChannelStore::Vec2(map) => map.get(k).copied(),
    _ => unreachable!(),
  }
}

fn set_uv(m: &mut LinkedMesh<()>, k: VertexKey, v: [f32; 2]) {
  match &mut m.vertex_channels.get_mut("uv").unwrap().store {
    ChannelStore::Vec2(map) => {
      map.insert(k, v);
    }
    _ => unreachable!(),
  }
}

fn tangent(m: &LinkedMesh<()>, k: VertexKey) -> Option<[f32; 3]> {
  match &m.vertex_channels["tangent"].store {
    ChannelStore::Vec3(map) => map.get(k).copied(),
    _ => unreachable!(),
  }
}

fn set_tangent(m: &mut LinkedMesh<()>, k: VertexKey, v: [f32; 3]) {
  match &mut m.vertex_channels.get_mut("tangent").unwrap().store {
    ChannelStore::Vec3(map) => {
      map.insert(k, v);
    }
    _ => unreachable!(),
  }
}

/// Exercises the generic passive-channel registry through its public propagation surface:
/// interpolated/cloned construction, surface flip (per-channel FlipXform), and removal sweep.
#[test]
fn passive_channels_propagate_and_flip() {
  let mut m: LinkedMesh<()> = LinkedMesh::new_box(2., 2., 2.);
  m.vertex_channels
    .insert("uv".into(), Channel::new(Arity::Vec2, Interp::Lerp, FlipXform::Identity));
  m.vertex_channels.insert(
    "tangent".into(),
    Channel::new(Arity::Vec3, Interp::LerpNormalize, FlipXform::Negate),
  );

  let keys: Vec<VertexKey> = m.vertices.keys().collect();
  for (i, &k) in keys.iter().enumerate() {
    set_uv(&mut m, k, [i as f32, 0.]);
    set_tangent(&mut m, k, [1., 0., 0.]);
  }
  let (a, b) = (keys[0], keys[1]);

  // interpolate: midpoint of vertices 0 ([0,0]) and 1 ([1,0]) → [0.5,0]; tangents renormalize.
  let mid = m.add_vertex_interpolated(&[(a, 0.5), (b, 0.5)], Vec3::zeros());
  assert_eq!(uv(&m, mid), Some([0.5, 0.]));
  assert_eq!(tangent(&m, mid), Some([1., 0., 0.]));

  // clone: duplicate inherits the source's channel values verbatim.
  let cloned = m.add_vertex_cloned_from(a, Vec3::zeros());
  assert_eq!(uv(&m, cloned), uv(&m, a));
  assert_eq!(tangent(&m, cloned), tangent(&m, a));

  // flip: Negate channel (tangent) flips, Identity channel (uv) is untouched.
  let uv_a = uv(&m, a);
  m.flip_normals();
  assert_eq!(uv(&m, a), uv_a, "uv (Identity) must survive flip unchanged");
  assert_eq!(tangent(&m, a), Some([-1., 0., 0.]), "tangent (Negate) must flip");

  // removal sweeps the registry too.
  m.remove_vertex(a);
  assert!(uv(&m, a).is_none() && tangent(&m, a).is_none());
}

/// End-to-end wiring check: the seam-split inside `separate_vertices_and_compute_normals` must
/// route through the cloning constructor, so every split duplicate carries the channel value.
#[test]
fn separate_vertices_clones_passive_channels_onto_split_duplicates() {
  let mut m: LinkedMesh<()> = LinkedMesh::new_box(2., 2., 2.);
  m.vertex_channels
    .insert("uv".into(), Channel::new(Arity::Vec2, Interp::Lerp, FlipXform::Identity));
  for k in m.vertices.keys().collect::<Vec<_>>() {
    set_uv(&mut m, k, [7., 9.]);
  }

  m.mark_edge_sharpness(0.8);
  m.separate_vertices_and_compute_normals();

  // all-sharp box: 8 corners each split into 3 → 24 verts, every one carrying the cloned uv.
  assert_eq!(m.vertices.len(), 24);
  for k in m.vertices.keys() {
    assert_eq!(uv(&m, k), Some([7., 9.]), "split duplicate missing cloned uv");
  }
}
