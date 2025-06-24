use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{FaceKey, Vec3, Vertex},
  LinkedMesh,
};

fn extrude_single_component(mesh: &mut LinkedMesh<()>, up: Vec3, faces: &[FaceKey]) {
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
    let new_vtx_keys = {
      let orig = mesh.faces[face_key].vertices;
      [orig[2], orig[1], orig[0]].map(|vtx_key| {
        *new_vtx_key_by_old.entry(vtx_key).or_insert_with(|| {
          mesh.vertices.insert(Vertex {
            position: mesh.vertices[vtx_key].position + up,
            shading_normal: None,
            displacement_normal: None,
            edges: Vec::new(),
          })
        })
      })
    };
    mesh.add_face(new_vtx_keys, ());
  }

  let mut visited = FxHashSet::default();
  for &start in &border_edges {
    for start in mesh.edges[start].vertices {
      if visited.contains(&start) {
        continue;
      }
      visited.insert(start);
      let mut cur = start;

      // Walk until we loop or hit a dead end (shouldn't happen for a proper border loop)
      while let Some(next) = mesh.vertices[cur]
        .edges
        .iter()
        .filter(|&edge_key| border_edges.contains(edge_key))
        .find_map(|&edge_key| {
          let edge = &mesh.edges[edge_key];
          let pair_vtx_key = if edge.vertices[0] == cur {
            edge.vertices[1]
          } else {
            edge.vertices[0]
          };

          if pair_vtx_key == start && cur != start {
            return Some(start);
          } else if visited.contains(&pair_vtx_key) {
            return None;
          }

          let face = &mesh.faces[edge.faces[0]];
          let is_backwards = if face.vertices[0] == cur {
            face.vertices[2] == pair_vtx_key
          } else if face.vertices[1] == cur {
            face.vertices[0] == pair_vtx_key
          } else if face.vertices[2] == cur {
            face.vertices[1] == pair_vtx_key
          } else {
            unreachable!()
          };

          if is_backwards {
            return None;
          }

          Some(pair_vtx_key)
        })
      {
        let v0 = cur;
        let v1 = next;
        let nv0 = new_vtx_key_by_old[&v0];
        let nv1 = new_vtx_key_by_old[&v1];
        mesh.add_face([nv1, v1, v0], ());
        mesh.add_face([nv0, nv1, v0], ());

        visited.insert(next);
        cur = next;
      }
    }
  }
}

pub fn extrude(mesh: &mut LinkedMesh<()>, up: Vec3) {
  let components = mesh.connected_components();
  for faces in components {
    extrude_single_component(mesh, up, &faces);
  }
}
