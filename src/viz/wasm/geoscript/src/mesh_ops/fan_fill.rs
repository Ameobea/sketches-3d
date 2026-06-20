use mesh::{
  linked_mesh::{Vec3, Vertex},
  LinkedMesh,
};

use crate::ErrorStack;

fn fan_fill_into(
  mesh: &mut LinkedMesh<()>,
  path: &[Vec3],
  closed: bool,
  flipped: bool,
  center: Option<Vec3>,
) {
  if path.len() < 2 {
    return;
  }

  let center = center.unwrap_or_else(|| {
    path.iter().fold(Vec3::new(0., 0., 0.), |acc, v| acc + *v) / path.len() as f32
  });
  let center_vtx_key = mesh.vertices.insert(Vertex::new(center));

  let start_vtx_key = mesh.vertices.insert(Vertex::new(path[0]));
  let mut vtx0_key = start_vtx_key;
  for vtx1_ix in 1..path.len() {
    let vtx1_key = mesh.vertices.insert(Vertex::new(path[vtx1_ix]));
    let tri = if flipped {
      [vtx1_key, vtx0_key, center_vtx_key]
    } else {
      [center_vtx_key, vtx0_key, vtx1_key]
    };
    mesh.add_face::<true>(tri, ());
    vtx0_key = vtx1_key;
  }

  if closed {
    let tri = if flipped {
      [start_vtx_key, vtx0_key, center_vtx_key]
    } else {
      [center_vtx_key, vtx0_key, start_vtx_key]
    };
    mesh.add_face::<true>(tri, ());
  }
}

pub fn fan_fill(
  path: &[Vec3],
  closed: bool,
  flipped: bool,
  center: Option<Vec3>,
) -> Result<LinkedMesh<()>, ErrorStack> {
  let mut mesh = LinkedMesh::new(0, 0, None);
  fan_fill_into(&mut mesh, path, closed, flipped, center);
  Ok(mesh)
}

pub fn fan_fill_subpaths(
  subpaths: &[(Vec<Vec3>, bool)],
  flipped: bool,
  center: Option<Vec3>,
) -> Result<LinkedMesh<()>, ErrorStack> {
  let mut mesh = LinkedMesh::new(0, 0, None);
  for (path, closed) in subpaths {
    fan_fill_into(&mut mesh, path, *closed, flipped, center);
  }
  Ok(mesh)
}
