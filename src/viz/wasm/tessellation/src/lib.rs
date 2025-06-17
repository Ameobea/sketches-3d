#![feature(iter_array_chunks)]

use float_ord::FloatOrd;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, Edge, EdgeSplitPos, Face},
  LinkedMesh,
};

fn does_face_have_edge_needing_split<T>(
  face: &Face<T>,
  mesh: &LinkedMesh<T>,
  should_split_edge: &impl Fn(&LinkedMesh<T>, &Edge) -> bool,
) -> bool {
  face
    .edges
    .iter()
    .map(|&edge_key| &mesh.edges[edge_key])
    .any(|edge| should_split_edge(mesh, edge))
}

pub fn tessellate_mesh_cb<T: Default>(
  mesh: &mut LinkedMesh<T>,
  displacement_normal_method: DisplacementNormalMethod,
  should_split_edge: &impl Fn(&LinkedMesh<T>, &Edge) -> bool,
) {
  let mut face_keys_needing_tessellation: Vec<_> = mesh
    .faces
    .iter()
    .filter_map(|(face_key, face)| {
      let has_edge_needing_split = does_face_have_edge_needing_split(face, mesh, should_split_edge);
      if has_edge_needing_split {
        Some(face_key)
      } else {
        None
      }
    })
    .collect();

  if face_keys_needing_tessellation.is_empty() {
    return;
  }

  let mut edges_needing_split = Vec::new();
  while !face_keys_needing_tessellation.is_empty() {
    while let Some(face_key) = face_keys_needing_tessellation.pop() {
      let Some(face) = mesh.faces.get(face_key) else {
        // This face might have already been split and removed from the mesh
        continue;
      };

      // Check again to see if the face still needs tessellation
      let still_needs_split = does_face_have_edge_needing_split(face, mesh, should_split_edge);
      if !still_needs_split {
        continue;
      }

      let mut has_bad_edge = false;
      let longest_edge_key = face
        .edges
        .iter()
        .map(|&edge_key| edge_key)
        .max_by_key(|&edge_key| {
          let edge = &mesh.edges[edge_key];
          let length = edge.length(&mesh.vertices);
          if length.is_nan() || length.is_infinite() {
            has_bad_edge = true;
          }
          FloatOrd(length)
        })
        .unwrap();
      if !has_bad_edge {
        edges_needing_split.push(longest_edge_key);
      }
    }

    if edges_needing_split.is_empty() {
      return;
    }
    while let Some(edge_key) = edges_needing_split.pop() {
      if mesh.edges.get(edge_key).is_none() {
        // This edge might have already been split and removed from the mesh
        continue;
      }

      mesh.split_edge_cb(
        edge_key,
        EdgeSplitPos::middle(),
        displacement_normal_method,
        |mesh, _old_face_key, _face_data, new_face_keys| {
          for face_key in new_face_keys {
            let face = &mesh.faces[face_key];
            if does_face_have_edge_needing_split(face, mesh, should_split_edge) {
              face_keys_needing_tessellation.push(face_key);
            }
          }
        },
      );
    }
  }
}

pub fn tessellate_mesh<T: Default>(
  mesh: &mut LinkedMesh<T>,
  target_edge_length: f32,
  displacement_normal_method: DisplacementNormalMethod,
) {
  let should_split_edge = |mesh: &LinkedMesh<T>, edge: &Edge| -> bool {
    let length = edge.length(&mesh.vertices);
    if length.is_nan() || length.is_infinite() {
      return false;
    }
    let split_length = length / 2.;
    // if the post-split length would be closer to the target length than the
    // current length, then we need to split this edge
    (split_length - target_edge_length).abs() < (length - target_edge_length).abs()
  };
  tessellate_mesh_cb(mesh, displacement_normal_method, &should_split_edge);
}

#[test]
fn tessellate_sanity() {
  let indices = [0, 1, 2];
  let vertices = [0., 0., 0., 1., 0., 0., 0., 1., 0.];

  let mut mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(&vertices, &indices, None, None);
  tessellate_mesh(&mut mesh, 0.1, DisplacementNormalMethod::EdgeNormal);

  let raw = mesh.to_raw_indexed(false, false, false);
  dbg!(&raw.vertices);
  dbg!(&raw.indices);
}
