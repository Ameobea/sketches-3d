use fxhash::FxHashMap;
use nalgebra::{Matrix4, Vector3};
use slotmap::{new_key_type, Key, SlotMap};
use smallvec::SmallVec;

use super::OwnedIndexedMesh;

type Vec3 = Vector3<f32>;
type Mat4 = Matrix4<f32>;

new_key_type! {
  pub struct VertexKey;
  pub struct FaceKey;
  pub struct EdgeKey;
}

pub struct Vertex {
  pub position: Vec3,
  pub normal: Option<Vec3>,
  edges: SmallVec<[EdgeKey; 2]>,
}

pub struct Face {
  /// Counter-clockwise winding
  pub vertices: [VertexKey; 3],
  /// Unordered
  pub edges: [EdgeKey; 3],
}

impl Face {
  pub fn vertex_positions(&self, verts: &SlotMap<VertexKey, Vertex>) -> [Vec3; 3] {
    [
      verts[self.vertices[0]].position,
      verts[self.vertices[1]].position,
      verts[self.vertices[2]].position,
    ]
  }

  pub fn normal(&self, verts: &SlotMap<VertexKey, Vertex>) -> Vector3<f32> {
    let [a, b, c] = self.vertex_positions(verts);
    (b - a).cross(&(c - a)).normalize()
  }

  pub fn area(&self, verts: &SlotMap<VertexKey, Vertex>) -> f32 {
    let [a, b, c] = self.vertex_positions(verts);
    0.5 * (b - a).cross(&(c - a)).magnitude()
  }

  pub fn center(&self, verts: &SlotMap<VertexKey, Vertex>) -> Vector3<f32> {
    let [a, b, c] = self.vertex_positions(verts);
    (a + b + c) / 3.
  }
}

pub struct Edge {
  // Ordered such that the first vertex key is always less than the second
  pub vertices: [VertexKey; 2],
  pub faces: SmallVec<[FaceKey; 2]>,
}

impl Edge {
  pub fn length(&self, verts: &SlotMap<VertexKey, Vertex>) -> f32 {
    let a = verts[self.vertices[0]].position;
    let b = verts[self.vertices[1]].position;
    (a - b).magnitude()
  }
}

/// Mesh representation that maintains topological information between vertices,
/// edges, and faces.
///
/// Similar to a half-edge data structure, but with a some differences such as
/// all faces being triangles and no intrinsic directionality to edges.
/// Instead, faces store their vertices directly in-order.
pub struct LinkedMesh {
  pub vertices: SlotMap<VertexKey, Vertex>,
  pub faces: SlotMap<FaceKey, Face>,
  pub edges: SlotMap<EdgeKey, Edge>,
  pub transform: Option<Mat4>,
}

fn sort_edge(v0: VertexKey, v1: VertexKey) -> [VertexKey; 2] {
  if v0 < v1 {
    [v0, v1]
  } else {
    [v1, v0]
  }
}

impl LinkedMesh {
  pub fn new(vertex_count: usize, face_count: usize, transform: Option<Mat4>) -> Self {
    Self {
      vertices: SlotMap::with_capacity_and_key(vertex_count),
      faces: SlotMap::with_capacity_and_key(face_count),
      edges: SlotMap::with_key(),
      transform,
    }
  }

  pub fn iter_faces(&self) -> impl Iterator<Item = (FaceKey, &Face)> {
    self.faces.iter()
  }

  pub fn iter_edges(&self) -> impl Iterator<Item = (EdgeKey, &Edge)> {
    self.edges.iter()
  }

  pub fn iter_vertices(&self) -> impl Iterator<Item = (VertexKey, &Vertex)> {
    self.vertices.iter()
  }

  pub fn from_triangles(
    vertices: &[Vec3],
    indices: &[usize],
    normals: Option<&[Vec3]>,
    transform: Option<Mat4>,
  ) -> Self {
    let mut mesh = Self::new(vertices.len(), indices.len() / 3, transform);

    let vertex_keys_by_ix = vertices
      .iter()
      .enumerate()
      .map(|(i, &position)| {
        mesh.vertices.insert(Vertex {
          position,
          normal: normals.map(|normals| normals[i]),
          edges: SmallVec::new(),
        })
      })
      .collect::<Vec<_>>();

    for [&a_ix, &b_ix, &c_ix] in indices.iter().array_chunks::<3>() {
      let a = vertex_keys_by_ix[a_ix];
      let b = vertex_keys_by_ix[b_ix];
      let c = vertex_keys_by_ix[c_ix];

      mesh.add_face([a, b, c]);
    }

    mesh
  }

  pub fn from_raw_indexed(
    vertices: &[f32],
    indices: &[usize],
    normals: Option<&[f32]>,
    transform: Option<Mat4>,
  ) -> Self {
    let vertices =
      unsafe { std::slice::from_raw_parts(vertices.as_ptr() as *const Vec3, vertices.len() / 3) };
    let normals = if let Some(normals) = normals {
      Some(unsafe {
        std::slice::from_raw_parts(normals.as_ptr() as *const Vec3, normals.len() / 3)
      })
    } else {
      None
    };

    Self::from_triangles(vertices, indices, normals, transform)
  }

  pub fn to_raw_indexed(&self) -> OwnedIndexedMesh {
    let mut vertices = Vec::with_capacity(self.vertices.len() * 3);
    let mut normals = None;
    let mut indices = Vec::with_capacity(self.faces.len() * 3);

    let mut cur_vert_ix = 0;
    let mut seen_vertex_keys =
      FxHashMap::with_capacity_and_hasher(self.vertices.len(), fxhash::FxBuildHasher::default());

    for face in self.faces.values() {
      for &vert_key in &face.vertices {
        let vert = &self.vertices[vert_key];
        let vert_ix = *seen_vertex_keys.entry(vert_key).or_insert_with(|| {
          let ix = cur_vert_ix;
          vertices.extend(vert.position.iter());
          cur_vert_ix += 1;
          ix
        });
        indices.push(vert_ix);

        if let Some(normal) = vert.normal {
          let normals = normals.get_or_insert_with(|| Vec::with_capacity(self.vertices.len() * 3));
          normals.extend(normal.iter());
        }
      }
    }

    OwnedIndexedMesh {
      vertices,
      normals,
      indices,
      transform: self.transform.clone(),
    }
  }

  /// Splits an edge in half, creating a new vertex in the middle.  All faces
  /// that include this edge are split into two faces with new edges being
  /// created for each.
  pub fn split_edge(&mut self, edge_key_to_split: EdgeKey) {
    let (edge_id_to_split, vm_position, vm_normal, faces_to_split) = {
      let edge = self.edges.get(edge_key_to_split).unwrap_or_else(|| {
        panic!("Tried to split edge that doesn't exist; key={edge_key_to_split:?}")
      });
      let edge_id_to_split = edge.vertices;
      let [v0_key, v1_key] = edge_id_to_split;
      let [v0, v1] = [&self.vertices[v0_key], &self.vertices[v1_key]];

      // Create new vertex at midpoint
      let vm_position = (v0.position + v1.position) * 0.5;
      let vm_normal = match (v0.normal, v1.normal) {
        (Some(n0), Some(n1)) => Some((n0 + n1).normalize()),
        _ => None,
      };

      let faces_to_split = edge.faces.clone();

      (edge_id_to_split, vm_position, vm_normal, faces_to_split)
    };

    let vm_key = self.vertices.insert(Vertex {
      position: vm_position,
      normal: vm_normal,
      edges: SmallVec::new(),
    });

    // Split each adjacent face
    let mut new_faces = SmallVec::<[_; 8]>::new();
    for face_key in faces_to_split {
      let old_face = &self.faces[face_key];
      let [v0_ix, v1_ix, v2_ix] = old_face.vertices;
      let edge_ids: [[VertexKey; 2]; 3] = [
        sort_edge(v0_ix, v1_ix),
        sort_edge(v1_ix, v2_ix),
        sort_edge(v2_ix, v0_ix),
      ];
      let split_edge_ix = edge_ids
        .iter()
        .position(|&id| id == edge_id_to_split)
        .unwrap();
      let (new_face_0_verts, new_face_1_verts) = match split_edge_ix {
        0 => ([v0_ix, vm_key, v2_ix], [vm_key, v1_ix, v2_ix]),
        1 => ([v0_ix, v1_ix, vm_key], [v0_ix, vm_key, v2_ix]),
        2 => ([v0_ix, v1_ix, vm_key], [v1_ix, v2_ix, vm_key]),
        _ => unreachable!(),
      };
      new_faces.push(new_face_0_verts);
      new_faces.push(new_face_1_verts);

      self.remove_face(face_key, true);
    }

    for verts in new_faces {
      self.add_face(verts);
    }

    self.edges.remove(edge_key_to_split);
  }

  fn add_edge(&mut self, vertices: [VertexKey; 2], faces: SmallVec<[FaceKey; 2]>) -> EdgeKey {
    let sorted_vertex_keys = sort_edge(vertices[0], vertices[1]);
    match self.vertices[vertices[0]].edges.iter().find(|&&edge_key| {
      let edge = &self.edges[edge_key];
      edge.vertices == sorted_vertex_keys
    }) {
      Some(&edge_key) => edge_key,
      None => {
        let edge_key = self.edges.insert(Edge {
          vertices: sorted_vertex_keys,
          faces,
        });

        for &vert_key in &vertices {
          let vert = &mut self.vertices[vert_key];
          vert.edges.push(edge_key);
        }

        edge_key
      }
    }
  }

  fn add_face(&mut self, vertices: [VertexKey; 3]) -> FaceKey {
    let edges = [
      sort_edge(vertices[0], vertices[1]),
      sort_edge(vertices[1], vertices[2]),
      sort_edge(vertices[2], vertices[0]),
    ];
    let mut edge_keys: [EdgeKey; 3] = [EdgeKey::null(); 3];
    for (i, &[v0, v1]) in edges.iter().enumerate() {
      let edge_key = self.add_edge([v0, v1], SmallVec::new());
      edge_keys[i] = edge_key;
    }

    let face_key = self.faces.insert(Face {
      vertices,
      edges: edge_keys,
    });

    for edge_key in edge_keys {
      let edge = &mut self.edges[edge_key];
      edge.faces.push(face_key);

      for vert in edge.vertices {
        let vert = &mut self.vertices[vert];
        if !vert.edges.contains(&edge_key) {
          vert.edges.push(edge_key);
        }
      }
    }

    face_key
  }

  fn remove_face(&mut self, face_key: FaceKey, preserve_verts: bool) {
    let face = self
      .faces
      .remove(face_key)
      .unwrap_or_else(|| panic!("Tried to remove face that doesn't exist; key={face_key:?}"));

    for edge_key in face.edges {
      let edge = &mut self.edges[edge_key];
      edge.faces.retain(|&mut f| f != face_key);
      if edge.faces.is_empty() {
        let edge = self.edges.remove(edge_key).unwrap_or_else(|| {
          panic!(
            "Tried to remove edge that doesn't exist; key={edge_key:?}. \
            Was referenced by removed face with key={face_key:?}",
          )
        });

        for vert_key in edge.vertices {
          let vert = &mut self.vertices[vert_key];
          vert.edges.retain(|&mut e| e != edge_key);

          if !preserve_verts && vert.edges.is_empty() {
            self.vertices.remove(vert_key);
          }
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn basic_edge_split() {
    let verts = [
      Vec3::new(-1., 0., 0.),
      Vec3::new(0., 1., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(0., -1., 0.),
    ];

    // 1 -> 0 -> 3
    // 2 -> 1 -> 3
    let indices = [1, 0, 3, 2, 1, 3];

    let mut mesh = LinkedMesh::from_triangles(&verts, &indices, None, None);
    let middle_edge_key = mesh
      .iter_edges()
      .find(|(_, e)| e.faces.len() == 2)
      .unwrap()
      .0;
    mesh.split_edge(middle_edge_key);

    assert_eq!(mesh.iter_vertices().count(), 5);
    let center_vtx_key = mesh
      .iter_vertices()
      .find(|(_, v)| v.position == Vec3::zeros())
      .unwrap()
      .0;
    let center_vtx = &mesh.vertices[center_vtx_key];
    assert_eq!(center_vtx.edges.len(), 4);

    for &edge_key in &center_vtx.edges {
      if edge_key == middle_edge_key {
        continue;
      }

      let edge = &mesh.edges[edge_key];
      assert!(edge.faces.len() <= 2);

      for vtx_key in edge.vertices {
        if vtx_key == center_vtx_key {
          continue;
        }

        let vtx = &mesh.vertices[vtx_key];
        assert!(vtx.edges.len() <= 3);
      }
    }

    /// Returns true if the two triangles have the same vertices, regardless of
    /// order (as long as the winding order is consistent)
    fn tris_equal(t0: [[i32; 3]; 3], t1: [[i32; 3]; 3]) -> bool {
      return t0 == t1 || t0 == [t1[1], t1[2], t1[0]] || t0 == [t1[2], t1[0], t1[1]];
    }

    let expected_unindexed_verts = [
      [[0, 0, 0], [0, 1, 0], [-1, 0, 0]],
      [[1, 0, 0], [0, 1, 0], [0, 0, 0]],
      [[1, 0, 0], [0, 0, 0], [0, -1, 0]],
      [[0, -1, 0], [0, 0, 0], [-1, 0, 0]],
    ];
    let actual_indexed_mesh = mesh.to_raw_indexed();
    assert_eq!(actual_indexed_mesh.indices.len(), 12);
    let actual_verts = actual_indexed_mesh
      .indices
      .chunks(3)
      .map(|v| {
        let v0 = &actual_indexed_mesh.vertices[(v[0] * 3)..(v[0] * 3 + 3)];
        let v1 = &actual_indexed_mesh.vertices[(v[1] * 3)..(v[1] * 3 + 3)];
        let v2 = &actual_indexed_mesh.vertices[(v[2] * 3)..(v[2] * 3 + 3)];
        [
          [v0[0] as i32, v0[1] as i32, v0[2] as i32],
          [v1[0] as i32, v1[1] as i32, v1[2] as i32],
          [v2[0] as i32, v2[1] as i32, v2[2] as i32],
        ]
      })
      .collect::<Vec<_>>();

    assert_eq!(actual_verts.len(), expected_unindexed_verts.len());
    for expected_tri in &expected_unindexed_verts {
      assert!(
        actual_verts
          .iter()
          .any(|actual_tri| tris_equal(*actual_tri, *expected_tri)),
        "Expected triangle {:?} not found in actual triangles: {:?}",
        expected_tri,
        actual_verts
      );
    }
  }
}
