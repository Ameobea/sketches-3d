use mesh::{
  linked_mesh::{Vec3, Vertex, VertexKey},
  LinkedMesh,
};

/// Sweeps each subpath polyline along `up`, producing a quad strip per subpath; closed subpaths
/// wrap into a loop.
pub fn extrude_path(subpaths: Vec<(Vec<Vec3>, bool)>, up: Vec3, flipped: bool) -> LinkedMesh<()> {
  let mut mesh = LinkedMesh::new(0, 0, None);

  let push_vtx =
    |mesh: &mut LinkedMesh<()>, pos: Vec3| -> VertexKey { mesh.vertices.insert(Vertex::new(pos)) };

  for (points, closed) in subpaths {
    let n = points.len();
    if n < 2 + closed as usize {
      continue;
    }

    let bottom: Vec<VertexKey> = points.iter().map(|p| push_vtx(&mut mesh, *p)).collect();
    let top: Vec<VertexKey> = points
      .iter()
      .map(|p| push_vtx(&mut mesh, *p + up))
      .collect();

    for i in 0..if closed { n } else { n - 1 } {
      let j = (i + 1) % n;
      let (b0, b1, t0, t1) = (bottom[i], bottom[j], top[i], top[j]);
      let (tri0, tri1) = if flipped {
        ([b0, t1, b1], [b0, t0, t1])
      } else {
        ([b1, t1, b0], [t1, t0, b0])
      };
      mesh.add_face::<true>(tri0, ());
      mesh.add_face::<true>(tri1, ());
    }
  }

  mesh
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
    let mesh = extrude_path(vec![(points, false)], Vec3::new(0., 1., 0.), false);
    assert_eq!(mesh.vertices.len(), 6);
    assert_eq!(mesh.faces.len(), 4);
  }

  #[test]
  fn multiple_subpaths() {
    let a = vec![Vec3::new(0., 0., 0.), Vec3::new(1., 0., 0.)];
    let b = vec![
      Vec3::new(0., 0., 2.),
      Vec3::new(1., 0., 2.),
      Vec3::new(2., 0., 2.),
    ];
    let mesh = extrude_path(vec![(a, false), (b, false)], Vec3::new(0., 1., 0.), false);
    assert_eq!(mesh.vertices.len(), 4 + 6);
    assert_eq!(mesh.faces.len(), 2 + 4);
  }
}
