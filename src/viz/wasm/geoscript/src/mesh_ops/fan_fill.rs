use mesh::{
  linked_mesh::{Vec3, Vertex},
  LinkedMesh,
};

use crate::ErrorStack;

pub fn fan_fill(
  path: &[Vec3],
  closed: bool,
  flipped: bool,
  center: Option<Vec3>,
) -> Result<LinkedMesh<()>, ErrorStack> {
  let mut mesh = LinkedMesh::new(0, 0, None);
  if path.is_empty() {
    return Ok(mesh);
  }

  let center = center.unwrap_or_else(|| {
    path.iter().fold(Vec3::new(0., 0., 0.), |acc, v| acc + *v) / path.len() as f32
  });
  let center_vtx_key = mesh.vertices.insert(Vertex {
    position: center,
    shading_normal: None,
    displacement_normal: None,
    edges: Vec::new(),
  });

  let start_vtx_key = mesh.vertices.insert(Vertex {
    position: path[0],
    shading_normal: None,
    displacement_normal: None,
    edges: Vec::new(),
  });
  let mut vtx0_key = start_vtx_key;
  for vtx1_ix in 1..path.len() {
    let vtx1_key = mesh.vertices.insert(Vertex {
      position: path[vtx1_ix],
      shading_normal: None,
      displacement_normal: None,
      edges: Vec::new(),
    });
    let tri = if flipped {
      [vtx1_key, vtx0_key, center_vtx_key]
    } else {
      [center_vtx_key, vtx0_key, vtx1_key]
    };
    mesh.add_face(tri, ());
    vtx0_key = vtx1_key;
  }

  if closed {
    let tri = if flipped {
      [start_vtx_key, vtx0_key, center_vtx_key]
    } else {
      [center_vtx_key, vtx0_key, start_vtx_key]
    };
    mesh.add_face(tri, ());
  }

  Ok(mesh)
}
