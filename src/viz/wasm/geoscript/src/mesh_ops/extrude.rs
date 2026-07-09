use std::collections::hash_map::Entry;

use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{FaceKey, Vec3, Vertex, VertexKey},
  LinkedMesh,
};

use crate::ErrorStack;

/// Duplicates each vertex in `faces` (offset by the corresponding entry in `offsets`), adds the
/// duplicated triangles, flips the originals to face the other way, and stitches the two layers
/// with side walls along boundary edges.
///
/// `offsets` must contain an entry for every vertex referenced by `faces`.
fn extrude_with_offsets(
  mesh: &mut LinkedMesh<()>,
  faces: &[FaceKey],
  offsets: &FxHashMap<VertexKey, Vec3>,
) {
  let mut border_edges = FxHashSet::default();
  for &face_key in faces {
    for &edge_key in &mesh.faces[face_key].edges {
      if mesh.edges[edge_key].faces.len() == 1 {
        border_edges.insert(edge_key);
      }
    }
  }

  let mut new_vtx_key_by_old = FxHashMap::default();
  for &face_key in faces {
    let mut new_vtx_keys: [VertexKey; 3] = unsafe { std::mem::transmute([(0u32, 0u32); 3]) };
    for (i, &vtx_key) in mesh.faces[face_key].vertices.iter().enumerate() {
      let new_vtx_key = match new_vtx_key_by_old.entry(vtx_key) {
        Entry::Occupied(o) => *o.get(),
        Entry::Vacant(v) => {
          let pos = mesh.vertices[vtx_key].position;
          let offset = offsets[&vtx_key];
          let new_vtx_key = mesh.vertices.insert(Vertex::new(pos + offset));
          v.insert(new_vtx_key);
          new_vtx_key
        }
      };
      new_vtx_keys[i] = new_vtx_key;
    }

    mesh.add_face::<false>(new_vtx_keys, ());

    // flip the winding order of the original faces to create the bottom of the extrusion
    let old_face = &mut mesh.faces[face_key];
    old_face.vertices.reverse();
  }

  for &border_edge in &border_edges {
    // figure out canonical direction for extrusion using faces from the pre-extruded mesh
    let edge = &mesh.edges[border_edge];
    let face0 = &mesh.faces[edge.faces[0]];
    let is_backwards = if face0.vertices[0] == edge.vertices[0] {
      face0.vertices[2] == edge.vertices[1]
    } else if face0.vertices[1] == edge.vertices[0] {
      face0.vertices[0] == edge.vertices[1]
    } else if face0.vertices[2] == edge.vertices[0] {
      face0.vertices[1] == edge.vertices[1]
    } else {
      unreachable!()
    };
    let (v0, v1) = if is_backwards {
      (edge.vertices[1], edge.vertices[0])
    } else {
      (edge.vertices[0], edge.vertices[1])
    };

    // join the two border edges with two triangles
    let nv0 = new_vtx_key_by_old[&v0];
    let nv1 = new_vtx_key_by_old[&v1];
    mesh.add_face::<false>([nv1, v1, v0], ());
    mesh.add_face::<false>([nv0, nv1, v0], ());
  }
}

/// Walks `faces` and computes a per-vertex normal as the cross-product-weighted average of
/// adjacent face normals.  Cross product magnitude is proportional to triangle area, so this is
/// area-weighted.  Returned normals are unit-length; zero-length accumulators (e.g. for vertices
/// on degenerate fans) collapse to a zero vector.
fn compute_area_weighted_vertex_normals(
  mesh: &LinkedMesh<()>,
  faces: &[FaceKey],
) -> FxHashMap<VertexKey, Vec3> {
  let mut accumulators: FxHashMap<VertexKey, Vec3> = FxHashMap::default();
  for &face_key in faces {
    let vs = mesh.faces[face_key].vertices;
    let p0 = mesh.vertices[vs[0]].position;
    let p1 = mesh.vertices[vs[1]].position;
    let p2 = mesh.vertices[vs[2]].position;
    let cross = (p1 - p0).cross(&(p2 - p0));
    for &vk in &vs {
      *accumulators.entry(vk).or_insert(Vec3::zeros()) += cross;
    }
  }
  for n in accumulators.values_mut() {
    let len = n.norm();
    if len > 1e-12 {
      *n /= len;
    } else {
      *n = Vec3::zeros();
    }
  }
  accumulators
}

fn build_offsets_per_vertex(
  mesh: &LinkedMesh<()>,
  faces: &[FaceKey],
  mut compute: impl FnMut(VertexKey, Vec3) -> Result<Vec3, ErrorStack>,
) -> Result<FxHashMap<VertexKey, Vec3>, ErrorStack> {
  let mut offsets: FxHashMap<VertexKey, Vec3> = FxHashMap::default();
  for &face_key in faces {
    for &vtx_key in &mesh.faces[face_key].vertices {
      if let Entry::Vacant(v) = offsets.entry(vtx_key) {
        let pos = mesh.vertices[vtx_key].position;
        v.insert(compute(vtx_key, pos)?);
      }
    }
  }
  Ok(offsets)
}

pub fn extrude(
  mesh: &mut LinkedMesh<()>,
  up: impl Fn(Vec3) -> Result<Vec3, ErrorStack>,
) -> Result<(), ErrorStack> {
  let components = mesh.connected_components();
  for faces in components {
    let offsets = build_offsets_per_vertex(mesh, &faces, |_, pos| up(pos))?;
    extrude_with_offsets(mesh, &faces, &offsets);
  }
  Ok(())
}

pub fn extrude_along_normals(
  mesh: &mut LinkedMesh<()>,
  distance: impl Fn(Vec3) -> Result<f32, ErrorStack>,
) -> Result<(), ErrorStack> {
  extrude_along_normals_with_normal_override(mesh, distance, |_, _| Ok(None))
}

/// Like `extrude_along_normals`, but `normal_override(vtx, area_weighted_normal)` may replace a
/// vertex's offset direction with a unit normal of its own (e.g. an exact analytic surface normal);
/// returning `None` keeps the area-weighted normal.  The area-weighted normal is handed in so the
/// caller can sign-align its replacement to it, keeping the offset side stable.
pub fn extrude_along_normals_with_normal_override(
  mesh: &mut LinkedMesh<()>,
  distance: impl Fn(Vec3) -> Result<f32, ErrorStack>,
  mut normal_override: impl FnMut(VertexKey, Vec3) -> Result<Option<Vec3>, ErrorStack>,
) -> Result<(), ErrorStack> {
  let components = mesh.connected_components();
  for faces in components {
    let normals = compute_area_weighted_vertex_normals(mesh, &faces);
    let offsets = build_offsets_per_vertex(mesh, &faces, |vk, pos| {
      let topo = normals.get(&vk).copied().unwrap_or_else(Vec3::zeros);
      let n = normal_override(vk, topo)?.unwrap_or(topo);
      Ok(n * distance(pos)?)
    })?;
    extrude_with_offsets(mesh, &faces, &offsets);
  }
  Ok(())
}

#[test]
fn test_extrude_issue() {
  let verts = &[
    0.012867972,
    1.0,
    0.08357865,
    -0.19497475,
    1.0,
    0.087867975,
    -0.05355338,
    1.0,
    -0.05355338,
    0.0,
    1.0,
    0.0,
    0.3,
    1.0,
    0.3,
  ];
  let indices = &[0, 1, 2, 0, 2, 3, 0, 3, 4, 0, 4, 1];

  let mut mesh = LinkedMesh::from_raw_indexed(verts, indices, None, None);
  dbg!(&mesh.faces);
  dbg!(&mesh.vertices.iter().collect::<Vec<_>>());
  mesh
    .check_is_manifold::<false>()
    .expect("not manifold before extrude");

  extrude(&mut mesh, |_| Ok(Vec3::new(0., 1., 0.))).unwrap();
  mesh.check_is_manifold::<true>().expect("not two-manifold");
}

#[test]
fn test_extrude_along_normals_flat_plane() {
  // Two triangles forming a unit square in the XZ plane; normal is +Y.
  // Extruding along normals by 1 should produce a 2-manifold box of unit thickness.
  let verts = &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 1.0];
  let indices = &[0u32, 1, 2, 0, 2, 3];
  let mut mesh = LinkedMesh::from_raw_indexed(verts, indices, None, None);
  mesh
    .check_is_manifold::<false>()
    .expect("not manifold before extrude");

  extrude_along_normals(&mut mesh, |_| Ok(1.0)).unwrap();
  mesh
    .check_is_manifold::<true>()
    .expect("not two-manifold after extrude_along_normals");

  // Top layer should sit at Y ~= ±1 depending on input winding; check that some vertex moved.
  let max_y = mesh
    .vertices
    .iter()
    .map(|(_, v)| v.position.y)
    .fold(f32::NEG_INFINITY, f32::max);
  let min_y = mesh
    .vertices
    .iter()
    .map(|(_, v)| v.position.y)
    .fold(f32::INFINITY, f32::min);
  assert!(
    (max_y - min_y - 1.0).abs() < 1e-4,
    "expected unit thickness between layers, got max={max_y} min={min_y}"
  );
}
