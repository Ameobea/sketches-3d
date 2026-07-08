use mesh::{
  linked_mesh::{Vec3, Vertex, VertexKey},
  LinkedMesh,
};

use crate::ErrorStack;

/// Sweeps each subpath polyline along `up`, producing a quad strip per subpath.
/// Subpaths marked closed return an error.
pub fn extrude_path(
  subpaths: Vec<(Vec<Vec3>, bool)>,
  up: Vec3,
  flipped: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  let mut mesh = LinkedMesh::new(0, 0, None);

  let push_vtx =
    |mesh: &mut LinkedMesh<()>, pos: Vec3| -> VertexKey { mesh.vertices.insert(Vertex::new(pos)) };

  for (subpath_ix, (points, is_closed)) in subpaths.into_iter().enumerate() {
    if is_closed {
      return Err(ErrorStack::new(format!(
        "`extrude_path` requires open paths; subpath {subpath_ix} is closed"
      )));
    }
    if points.len() < 2 {
      continue;
    }

    let bottom: Vec<VertexKey> = points.iter().map(|p| push_vtx(&mut mesh, *p)).collect();
    let top: Vec<VertexKey> = points
      .iter()
      .map(|p| push_vtx(&mut mesh, *p + up))
      .collect();

    for i in 0..points.len() - 1 {
      let b0 = bottom[i];
      let b1 = bottom[i + 1];
      let t0 = top[i];
      let t1 = top[i + 1];
      let (tri0, tri1) = if flipped {
        ([b0, t1, b1], [b0, t0, t1])
      } else {
        ([b1, t1, b0], [t1, t0, b0])
      };
      mesh.add_face::<true>(tri0, ());
      mesh.add_face::<true>(tri1, ());
    }
  }

  Ok(mesh)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn straight_segment() {
    let points = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(2., 0., 0.),
    ];
    let mesh = extrude_path(vec![(points, false)], Vec3::new(0., 1., 0.), false).unwrap();
    assert_eq!(mesh.vertices.len(), 6);
    assert_eq!(mesh.faces.len(), 4);
  }

  #[test]
  fn rejects_closed() {
    let points = vec![Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.)];
    let err = extrude_path(vec![(points, true)], Vec3::new(0., 1., 0.), false).unwrap_err();
    assert!(err.to_string().contains("closed"));
  }

  #[test]
  fn multiple_subpaths() {
    let a = vec![Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.)];
    let b = vec![
      Vec3::new(0., 0., 2.),
      Vec3::new(1., 0., 2.),
      Vec3::new(2., 0., 2.),
    ];
    let mesh = extrude_path(vec![(a, false), (b, false)], Vec3::new(0., 1., 0.), false).unwrap();
    assert_eq!(mesh.vertices.len(), 4 + 6);
    assert_eq!(mesh.faces.len(), 2 + 4);
  }
}
