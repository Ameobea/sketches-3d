#![feature(iter_array_chunks)]

#[cfg_attr(feature = "bindgen", macro_use)]
extern crate log;

use float_ord::FloatOrd;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, Face},
  LinkedMesh,
};

#[cfg(feature = "bindgen")]
mod interface;

fn get_face_has_edge_needing_split(
  face: &Face,
  mesh: &LinkedMesh,
  target_edge_length: f32,
) -> bool {
  face
    .edges
    .iter()
    .map(|&edge_key| &mesh.edges[edge_key])
    .any(|edge| {
      let length = edge.length(&mesh.vertices);
      let split_length = target_edge_length / 2.;
      // if the post-split length would be closer to the target length than the
      // current length, then we need to split this edge
      (length - split_length).abs() > (length - target_edge_length).abs()
    })
}

/// Returns `true` if at least one face was split
fn tessellate_one_iter(
  mesh: &mut LinkedMesh,
  target_edge_length: f32,
  displacement_normal_method: DisplacementNormalMethod,
) -> bool {
  let face_keys_needing_tessellation: Vec<_> = mesh
    .iter_faces()
    .filter_map(|(face_key, face)| {
      let has_edge_needing_split = get_face_has_edge_needing_split(face, mesh, target_edge_length);
      if has_edge_needing_split {
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
    let still_needs_split = get_face_has_edge_needing_split(face, mesh, target_edge_length);
    if !still_needs_split {
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

    mesh.split_edge(edge_key, displacement_normal_method);
  }

  if cfg!(debug_assertions) {
    for edge_key in mesh.edges.keys() {
      let edge = &mesh.edges[edge_key];
      for &face_key in &edge.faces {
        let face = &mesh.faces[face_key];
        for vtx_key in edge.vertices {
          if !face.vertices.contains(&vtx_key) {
            panic!(
              "Edge doesn't contain vertex in face; edge={edge:?}; face={face:?}; \
               vtx_key={vtx_key:?}",
            );
          }
        }
      }
    }
  }

  true
}

pub fn tessellate_mesh(
  mesh: &mut LinkedMesh,
  target_edge_length: f32,
  displacement_normal_method: DisplacementNormalMethod,
) {
  loop {
    let did_split = tessellate_one_iter(mesh, target_edge_length, displacement_normal_method);
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
  tessellate_mesh(&mut mesh, 0.1, DisplacementNormalMethod::EdgeNormal);

  let raw = mesh.to_raw_indexed();
  dbg!(&raw.vertices);
  dbg!(&raw.indices);
}
