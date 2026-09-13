use nalgebra::Vector3;

/// Collapses near-collinear triangles (height below `ratio` × longest edge, as CSG leaves behind)
/// by merging the endpoints of their shortest edge, then drops unreferenced vertices. Inside such
/// a face geometry-central's geodesic tracer finds no edge to exit through and aborts the kernel.
pub fn collapse_sliver_triangles(
  vertices: &[f32],
  indices: &[u32],
  ratio: f32,
) -> (Vec<f32>, Vec<u32>) {
  let pos = |i: u32| {
    let i = i as usize * 3;
    Vector3::new(vertices[i], vertices[i + 1], vertices[i + 2])
  };
  let mut parent: Vec<u32> = (0..(vertices.len() / 3) as u32).collect();
  fn find(parent: &mut [u32], mut i: u32) -> u32 {
    while parent[i as usize] != i {
      parent[i as usize] = parent[parent[i as usize] as usize];
      i = parent[i as usize];
    }
    i
  }

  let mut tris: Vec<[u32; 3]> = indices
    .chunks_exact(3)
    .map(|t| [t[0], t[1], t[2]])
    .collect();
  for _ in 0..4 {
    let mut merged = 0;
    for t in &tris {
      let [a, b, c] = t.map(|i| find(&mut parent, i));
      if a == b || b == c || c == a {
        continue;
      }
      let (pa, pb, pc) = (pos(a), pos(b), pos(c));
      let edges = [
        (a, b, (pb - pa).norm()),
        (b, c, (pc - pb).norm()),
        (c, a, (pa - pc).norm()),
      ];
      let longest = edges.iter().map(|e| e.2).fold(0f32, f32::max);
      let height = (pb - pa).cross(&(pc - pa)).norm() / longest;
      if longest > 0. && height < ratio * longest {
        let (u, v, _) = edges
          .into_iter()
          .min_by(|x, y| x.2.total_cmp(&y.2))
          .unwrap();
        parent[u.max(v) as usize] = u.min(v);
        merged += 1;
      }
    }
    if merged == 0 {
      break;
    }
    tris.retain_mut(|t| {
      *t = t.map(|i| find(&mut parent, i));
      t[0] != t[1] && t[1] != t[2] && t[2] != t[0]
    });
  }

  let mut new_ix = vec![u32::MAX; vertices.len() / 3];
  let mut out_verts = Vec::with_capacity(vertices.len());
  let mut out_ix = Vec::with_capacity(tris.len() * 3);
  for t in &tris {
    for &i in t {
      if new_ix[i as usize] == u32::MAX {
        new_ix[i as usize] = (out_verts.len() / 3) as u32;
        out_verts.extend_from_slice(&vertices[i as usize * 3..i as usize * 3 + 3]);
      }
      out_ix.push(new_ix[i as usize]);
    }
  }
  (out_verts, out_ix)
}

#[cfg(test)]
mod tests {
  use super::collapse_sliver_triangles;

  #[test]
  fn cap_triangle_collapses_without_opening_a_hole() {
    // Vertex 2 sits on the 0-1 edge: [0,1,2] is a sliver; its fan neighbours share vertex 2.
    let verts = [0., 0., 0., 1., 0., 0., 0.3, 1e-9, 0., 0.5, 1., 0.];
    let indices = [0, 1, 2, 1, 3, 2, 2, 3, 0];
    let (v, ix) = collapse_sliver_triangles(&verts, &indices, 1e-5);
    // 2 merges into 0 (its shortest edge); the sliver and the face across the collapsed edge go.
    assert_eq!(ix, vec![0, 1, 2]);
    assert_eq!(v, vec![1., 0., 0., 0.5, 1., 0., 0., 0., 0.]);
    // Healthy geometry is untouched.
    let (v2, ix2) = collapse_sliver_triangles(&verts[..9], &[0, 1, 2], 1e-12);
    assert_eq!((v2.len(), ix2), (9, vec![0, 1, 2]));
  }
}
