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

use mesh::linked_mesh::{LinkedMesh, Plane, Vec3};
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
