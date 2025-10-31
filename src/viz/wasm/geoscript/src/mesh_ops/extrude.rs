use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{FaceKey, Vec3, Vertex},
  LinkedMesh,
};
use smallvec::SmallVec;

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
      mesh.faces[face_key].vertices.map(|vtx_key| {
        *new_vtx_key_by_old.entry(vtx_key).or_insert_with(|| {
          mesh.vertices.insert(Vertex {
            position: mesh.vertices[vtx_key].position + up,
            shading_normal: None,
            displacement_normal: None,
            edges: SmallVec::new(),
            _padding: Default::default(),
          })
        })
      })
    };
    mesh.add_face::<false>(new_vtx_keys, ());

    // flip the winding order of the original faces to create the bottom of the extrusion
    let old_face = &mut mesh.faces[face_key];
    old_face.vertices.reverse();
  }

  // let mut visited = FxHashSet::default();
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

pub fn extrude(mesh: &mut LinkedMesh<()>, up: Vec3) {
  let components = mesh.connected_components();
  for faces in components {
    extrude_single_component(mesh, up, &faces);
  }
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

  extrude(&mut mesh, Vec3::new(0., 1., 0.));
  mesh.check_is_manifold::<true>().expect("not two-manifold");
}
