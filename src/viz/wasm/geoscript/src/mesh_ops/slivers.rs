use fxhash::FxHashSet;
use nalgebra::Vector3;
use smallvec::SmallVec;

type Faces = SmallVec<[u32; 8]>;

/// Merges vertices at bit-identical positions (attribute seams the boolean decode keeps split),
/// dropping faces that collapse. Returns `None` when welding would put more than two faces on an
/// edge, i.e. when the coincident sheets are genuinely distinct surfaces rather than one seam.
pub fn weld_coincident_vertices(vertices: &[f32], indices: &[u32]) -> Option<(Vec<f32>, Vec<u32>)> {
  let mut first: fxhash::FxHashMap<[u32; 3], u32> = fxhash::FxHashMap::default();
  let mut remap = Vec::with_capacity(vertices.len() / 3);
  let mut out_verts = Vec::with_capacity(vertices.len());
  for v in vertices.chunks_exact(3) {
    let key = [v[0].to_bits(), v[1].to_bits(), v[2].to_bits()];
    remap.push(*first.entry(key).or_insert_with(|| {
      out_verts.extend_from_slice(v);
      (out_verts.len() / 3 - 1) as u32
    }));
  }
  if out_verts.len() == vertices.len() {
    return Some((vertices.to_vec(), indices.to_vec()));
  }
  let mut out_ix = Vec::with_capacity(indices.len());
  let mut edge_faces: fxhash::FxHashMap<(u32, u32), u8> = fxhash::FxHashMap::default();
  for t in indices.chunks_exact(3) {
    let t = [
      remap[t[0] as usize],
      remap[t[1] as usize],
      remap[t[2] as usize],
    ];
    if t[0] == t[1] || t[1] == t[2] || t[2] == t[0] {
      continue;
    }
    for k in 0..3 {
      let (a, b) = (t[k].min(t[(k + 1) % 3]), t[k].max(t[(k + 1) % 3]));
      let n = edge_faces.entry((a, b)).or_default();
      *n += 1;
      if *n > 2 {
        return None;
      }
    }
    out_ix.extend_from_slice(&t);
  }
  Some((out_verts, out_ix))
}

/// Collapses near-collinear triangles (height below `ratio` × longest edge, as CSG leaves behind)
/// by merging the endpoints of their shortest edge, then drops unreferenced vertices. Inside such a
/// face geometry-central's geodesic tracer finds no edge to exit through and aborts the kernel.
///
/// Every merge is a manifold edge collapse: it needs the link condition (the endpoints' only
/// shared neighbours are the apexes of the faces on that edge, and the edge has at most two
/// faces) and must not pinch two boundary vertices together through an interior edge. Collapses
/// that would break 2-manifoldness are skipped, leaving that sliver alone.
pub fn collapse_sliver_triangles(
  vertices: &[f32],
  indices: &[u32],
  ratio: f32,
) -> (Vec<f32>, Vec<u32>) {
  let nv = vertices.len() / 3;
  let pos = |i: u32| {
    let i = i as usize * 3;
    Vector3::new(vertices[i], vertices[i + 1], vertices[i + 2])
  };
  let mut tris: Vec<[u32; 3]> = indices
    .chunks_exact(3)
    .map(|t| [t[0], t[1], t[2]])
    .collect();
  let mut incident: Vec<Faces> = vec![Faces::new(); nv];
  for (fi, t) in tris.iter().enumerate() {
    for &v in t {
      incident[v as usize].push(fi as u32);
    }
  }
  let alive = |t: &[u32; 3]| t[0] != t[1] && t[1] != t[2] && t[2] != t[0];
  let faces_on_edge = |incident: &[Faces], tris: &[[u32; 3]], u: u32, v: u32| -> Faces {
    incident[u as usize]
      .iter()
      .copied()
      .filter(|&f| alive(&tris[f as usize]) && tris[f as usize].contains(&v))
      .collect()
  };
  let neighbors = |incident: &[Faces], tris: &[[u32; 3]], u: u32| -> FxHashSet<u32> {
    incident[u as usize]
      .iter()
      .filter(|&&f| alive(&tris[f as usize]))
      .flat_map(|&f| tris[f as usize])
      .filter(|&w| w != u)
      .collect()
  };
  let is_boundary = |incident: &[Faces], tris: &[[u32; 3]], u: u32| -> bool {
    neighbors(incident, tris, u)
      .into_iter()
      .any(|w| faces_on_edge(incident, tris, u, w).len() == 1)
  };

  for _ in 0..4 {
    let mut merged = 0;
    for fi in 0..tris.len() {
      let t = tris[fi];
      if !alive(&t) {
        continue;
      }
      let (pa, pb, pc) = (pos(t[0]), pos(t[1]), pos(t[2]));
      let edges = [
        (t[0], t[1], (pb - pa).norm()),
        (t[1], t[2], (pc - pb).norm()),
        (t[2], t[0], (pa - pc).norm()),
      ];
      let longest = edges.iter().map(|e| e.2).fold(0f32, f32::max);
      if longest == 0. || (pb - pa).cross(&(pc - pa)).norm() / longest >= ratio * longest {
        continue;
      }
      let (u, v, _) = edges
        .into_iter()
        .min_by(|x, y| x.2.total_cmp(&y.2))
        .unwrap();
      let (u, v) = (u.min(v), u.max(v));

      let on_edge = faces_on_edge(&incident, &tris, u, v);
      if on_edge.len() > 2 {
        continue;
      }
      let apexes: FxHashSet<u32> = on_edge
        .iter()
        .flat_map(|&f| tris[f as usize])
        .filter(|&w| w != u && w != v)
        .collect();
      let shared: FxHashSet<u32> = neighbors(&incident, &tris, u)
        .intersection(&neighbors(&incident, &tris, v))
        .copied()
        .collect();
      if shared != apexes {
        continue;
      }
      if on_edge.len() == 2 && is_boundary(&incident, &tris, u) && is_boundary(&incident, &tris, v)
      {
        continue;
      }

      let moved = std::mem::take(&mut incident[v as usize]);
      for &f in &moved {
        for w in tris[f as usize].iter_mut() {
          if *w == v {
            *w = u;
          }
        }
        incident[u as usize].push(f);
      }
      merged += 1;
    }
    if merged == 0 {
      break;
    }
  }

  let mut new_ix = vec![u32::MAX; nv];
  let mut out_verts = Vec::with_capacity(vertices.len());
  let mut out_ix = Vec::with_capacity(indices.len());
  for t in tris.iter().filter(|t| alive(t)) {
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
  use super::{collapse_sliver_triangles, weld_coincident_vertices};
  use std::collections::HashMap;

  /// Every edge on at most two faces, no duplicate faces.
  fn is_two_manifold(ix: &[u32]) -> bool {
    let mut edges = HashMap::new();
    let mut faces = std::collections::HashSet::new();
    for t in ix.chunks_exact(3) {
      let mut key = [t[0], t[1], t[2]];
      key.sort();
      if !faces.insert(key) {
        return false;
      }
      for i in 0..3 {
        let (a, b) = (t[i].min(t[(i + 1) % 3]), t[i].max(t[(i + 1) % 3]));
        *edges.entry((a, b)).or_insert(0) += 1;
      }
    }
    edges.values().all(|&n| n <= 2)
  }

  #[test]
  fn cap_triangle_collapses_without_opening_a_hole() {
    // Vertex 2 sits on the 0-1 edge: [0,1,2] is a sliver; its fan neighbours share vertex 2.
    let verts = [0., 0., 0., 1., 0., 0., 0.3, 1e-9, 0., 0.5, 1., 0.];
    let indices = [0, 1, 2, 1, 3, 2, 2, 3, 0];
    let (v, ix) = collapse_sliver_triangles(&verts, &indices, 1e-5);
    // 2 merges into 0 (its shortest edge); the sliver and the face across the collapsed edge go.
    assert_eq!(ix, vec![0, 1, 2]);
    assert_eq!(v, vec![1., 0., 0., 0.5, 1., 0., 0., 0., 0.]);
    assert!(is_two_manifold(&ix));
    // Healthy geometry is untouched.
    let (v2, ix2) = collapse_sliver_triangles(&verts[..9], &[0, 1, 2], 1e-12);
    assert_eq!((v2.len(), ix2), (9, vec![0, 1, 2]));
  }

  #[test]
  fn welds_seams_but_not_distinct_touching_sheets() {
    // A quad split along its diagonal into two sheets with duplicated diagonal vertices.
    let verts = [
      0., 0., 0., 1., 0., 0., 1., 1., 0., 0., 0., 0., 1., 1., 0., 0., 1., 0.,
    ];
    let (v, ix) = weld_coincident_vertices(&verts, &[0, 1, 2, 3, 4, 5]).unwrap();
    assert!(is_two_manifold(&ix));
    assert_eq!((v.len() / 3, ix), (4, vec![0, 1, 2, 0, 2, 3]));
    // Three sheets meeting at one coincident edge: welding would make a non-manifold edge.
    let verts = [
      0., 0., 0., 1., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 1., 0.,
      0., 0., -1., 0.,
    ];
    assert!(weld_coincident_vertices(&verts, &[0, 1, 2, 3, 4, 5, 6, 7, 8]).is_none());
  }

  #[test]
  fn refuses_collapses_that_would_break_manifoldness() {
    // Two triangles [0,1,3] and [0,2,4] hang off a sliver [0,1,2] (2 nearly on edge 0-1): 0-2 is
    // its shortest edge, but 0 and 2 share neighbour 1 through faces that don't sit on edge 0-2,
    // so the link condition fails; nothing may change.
    let verts = [
      0., 0., 0., 1., 0., 0., 0.3, 1e-9, 0., 0.5, 1., 0., -1., 1., 0., 2., -1., 0.,
    ];
    let indices = [0, 1, 2, 0, 3, 1, 0, 2, 4, 1, 5, 2];
    let (_, ix) = collapse_sliver_triangles(&verts, &indices, 1e-5);
    assert_eq!(ix, indices.to_vec());

    // A strip whose sliver's shortest edge is interior while both endpoints lie on the boundary:
    // collapsing would pinch the two boundary loops into a non-manifold vertex.
    let verts = [
      0., 0., 0., 1., 0., 0., 0.5, 1e-9, 0., 0.5, 1., 0., 0.5, -1., 0.,
    ];
    let indices = [0, 2, 3, 2, 1, 3, 0, 4, 2, 2, 4, 1];
    let (_, ix) = collapse_sliver_triangles(&verts, &indices, 1e-5);
    assert!(is_two_manifold(&ix));
    assert_eq!(ix.len(), indices.len());
  }
}
