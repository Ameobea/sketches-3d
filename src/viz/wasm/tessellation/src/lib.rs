#![feature(iter_array_chunks)]

#[macro_use]
extern crate log;

use common::mesh::LinkedMesh;
use float_ord::FloatOrd;

mod interface;

/// Returns `true` if at least one face was split
fn tessellate_one_iter(mesh: &mut LinkedMesh, target_triangle_area: f32) -> bool {
  let face_keys_needing_tessellation: Vec<_> = mesh
    .iter_faces()
    .filter_map(|(face_key, face)| {
      let area = face.area(&mesh.vertices);
      if area > target_triangle_area {
        Some(face_key)
      } else {
        None
      }
    })
    .collect();

  if face_keys_needing_tessellation.is_empty() {
    return false;
  }

  let mut edges_needing_split = Vec::new();
  for face_key in face_keys_needing_tessellation {
    let Some(face) = mesh.faces.get(face_key) else {
      // This face might have already been split and removed from the mesh
      continue;
    };

    // Check again to see if the face still needs tessellation
    let area = face.area(&mesh.vertices);
    if area <= target_triangle_area {
      continue;
    }

    let longest_edge_key = face
      .edges
      .iter()
      .map(|&edge_key| edge_key)
      .max_by_key(|&edge_key| {
        let edge = &mesh.edges[edge_key];
        let length = edge.length(&mesh.vertices);
        FloatOrd(length)
      })
      .unwrap();
    edges_needing_split.push(longest_edge_key);
  }

  if edges_needing_split.is_empty() {
    return false;
  }
  for edge_key in edges_needing_split {
    if mesh.edges.get(edge_key).is_none() {
      // This edge might have already been split and removed from the mesh
      continue;
    }

    mesh.split_edge(edge_key);
  }

  // TODO DEBUG REMOVE
  for edge_key in mesh.edges.keys() {
    let edge = &mesh.edges[edge_key];
    for &face_key in &edge.faces {
      let face = &mesh.faces[face_key];
      for vtx_key in edge.vertices {
        if !face.vertices.contains(&vtx_key) {
          panic!(
          "Edge doesn't contain vertex in face; edge={edge:?}; face={face:?}; vtx_key={vtx_key:?}",
        );
        }
      }
    }
  }

  true
}

fn tessellate_mesh(mesh: &mut LinkedMesh, target_triangle_area: f32) {
  loop {
    let did_split = tessellate_one_iter(mesh, target_triangle_area);
    if !did_split {
      break;
    }
  }
}

#[test]
fn tessellate_sanity() {
  let indices = [0, 1, 2];
  let vertices = [0., 0., 0., 1., 0., 0., 0., 1., 0.];

  let mut mesh = LinkedMesh::from_raw_indexed(&vertices, &indices, None, None);
  tessellate_mesh(&mut mesh, 0.1);

  let raw = mesh.to_raw_indexed();
  dbg!(&raw.vertices);
  dbg!(&raw.indices);
}
