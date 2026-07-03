use mesh::linked_mesh::LinkedMesh;

// A box triangulated the way Manifold emits CSG output: the quad diagonals are chosen so some
// corners are touched by only a single triangle of a face. That lone triangle is a smooth fan
// bounded by two sharp edges, and the edge-driven fan walk used to orphan it — leaving its corner
// carrying an adjacent (perpendicular) face's normal, which showed up as triplanar streaks. Every
// triangle here lies in an axis plane, so all three of its corner normals must agree with each other.
#[test]
fn sharp_fan_split_does_not_orphan_single_triangle_fans() {
  #[rustfmt::skip]
  let verts: [f32; 24] = [
    -1., 0., -1.,  -1., 0., 1.,  -1., 4., -1.,  -1., 4., 1.,
     1., 0., -1.,   1., 0., 1.,   1., 4., -1.,   1., 4., 1.,
  ];
  #[rustfmt::skip]
  let tris: [u32; 36] = [
    2, 0, 1,  3, 1, 5,  2, 6, 0,  2, 3, 6,  2, 1, 3,  6, 4, 0,
    7, 5, 4,  6, 7, 4,  3, 5, 7,  3, 7, 6,  1, 0, 4,  5, 1, 4,
  ];

  let mut mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(&verts, &tris, None, None);
  mesh.mark_edge_sharpness(0.8);
  let out = mesh.separate_normals_and_finalize(true, false, false);
  let normals = out.shading_normals.expect("shading normals");
  let nrm = |i: usize| [normals[i * 3], normals[i * 3 + 1], normals[i * 3 + 2]];
  let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];

  for t in 0..out.indices.len() / 3 {
    let [a, b, c] = [out.indices[t * 3], out.indices[t * 3 + 1], out.indices[t * 3 + 2]];
    let [na, nb, nc] = [nrm(a), nrm(b), nrm(c)];
    let min_dot = dot(na, nb).min(dot(nb, nc)).min(dot(na, nc));
    assert!(min_dot > 0.999, "tri {t} corner normals disagree: {na:?} {nb:?} {nc:?}");
  }
}
