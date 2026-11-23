//! Offset directions for `displace` and the region extrusion behind `emboss`. Both treat
//! bit-coincident vertices (seam twins) as one surface point, so split seams stay closed.

use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{EdgeKey, FaceKey, Vec3, VertexKey},
  LinkedMesh,
};
use nalgebra::{Matrix3, Vector3};
use smallvec::SmallVec;

use crate::ErrorStack;

/// Caps the offset at knife edges, where the exact miter runs off to infinity.
const MAX_MITER: f64 = 3.;

/// Offset direction where faces with the given unit normals meet: the vector whose projection
/// on every normal is 1, so a constant height moves flat faces parallel to themselves across
/// creases. Solved only along the directions the normals really span: where they differ by less
/// than ~15° (a faceted smooth surface, the noise of boolean output) the exact answer is the far
/// away meeting point of nearly parallel planes, and the mean normal is used instead.
pub fn miter(normals: impl Iterator<Item = (Vec3, f32)>) -> Vec3 {
  const MIN_SPREAD: f64 = 0.02;
  let (mut a, mut b, mut w_sum) = (Matrix3::<f64>::zeros(), Vector3::<f64>::zeros(), 0.);
  for (n, w) in normals {
    let (n, w) = (n.cast::<f64>(), w as f64);
    a += n * n.transpose() * w;
    b += n * w;
    w_sum += w;
  }
  if w_sum <= 0. || b.norm() < 1e-6 * w_sum {
    return Vec3::zeros();
  }
  let (a, b) = (a / w_sum, b / w_sum);
  let eig = a.symmetric_eigen();
  (0..3)
    .filter(|&i| eig.eigenvalues[i] > MIN_SPREAD)
    .map(|i| {
      let v = eig.eigenvectors.column(i);
      v * (v.dot(&b) / eig.eigenvalues[i])
    })
    .sum::<Vector3<f64>>()
    .cap_magnitude(MAX_MITER)
    .cast()
}

/// `None` for slivers (boolean output is full of them), whose normals are rounding noise that a
/// miter would amplify.
fn unit_normal(m: &LinkedMesh<()>, f: FaceKey) -> Option<Vec3> {
  let [p, q, r] = m.faces[f].vertices.map(|k| m.vertices[k].position);
  let (a, b) = (q - p, r - p);
  let longest = a
    .norm_squared()
    .max(b.norm_squared())
    .max((r - q).norm_squared());
  a.cross(&b).try_normalize(longest * 1e-4)
}

pub fn face_dir(m: &LinkedMesh<()>, f: FaceKey) -> Vec3 {
  unit_normal(m, f).unwrap_or_default()
}

/// Sorted so every twin of a seam edge sums in the same order and lands on identical bits.
pub fn edge_dir(m: &LinkedMesh<()>, edges: &[EdgeKey]) -> Vec3 {
  let mut faces: SmallVec<[FaceKey; 4]> = edges
    .iter()
    .flat_map(|&e| m.edges[e].faces.iter().copied())
    .collect();
  faces.sort_unstable();
  miter(faces.iter().filter_map(|&f| Some((unit_normal(m, f)?, 1.))))
}

fn fan(m: &LinkedMesh<()>, v: VertexKey) -> SmallVec<[FaceKey; 12]> {
  let mut faces: SmallVec<[FaceKey; 12]> = m.vertices[v]
    .edges
    .iter()
    .flat_map(|&e| m.edges[e].faces.iter().copied())
    .collect();
  faces.sort_unstable();
  faces.dedup();
  faces
}

/// Miter over the faces around a group of coincident vertices that pass `keep`, weighted by
/// their corner angle so the result doesn't depend on how a flat face happens to be triangulated.
pub fn group_dir(m: &LinkedMesh<()>, group: &[VertexKey], keep: impl Fn(FaceKey) -> bool) -> Vec3 {
  miter(group.iter().flat_map(|&v| {
    let keep = &keep;
    fan(m, v)
      .into_iter()
      .filter(move |&f| keep(f))
      .filter_map(move |f| {
        Some((
          unit_normal(m, f)?,
          m.faces[f].compute_angle_at_vertex_key(v, &m.vertices),
        ))
      })
  }))
}

pub fn coincident_groups(m: &LinkedMesh<()>) -> Vec<SmallVec<[VertexKey; 2]>> {
  let mut ix: FxHashMap<[u32; 3], usize> = FxHashMap::default();
  let mut groups: Vec<SmallVec<[VertexKey; 2]>> = Vec::new();
  for (k, v) in m.vertices.iter() {
    let p = v.position;
    let i = *ix
      .entry([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
      .or_insert(groups.len());
    if i == groups.len() {
      groups.push(SmallVec::new());
    }
    groups[i].push(k);
  }
  groups
}

/// Moves the `inside` faces out by `height` along their mitered normals and bridges the gap to
/// the rest of the mesh with walls, so a closed mesh stays closed. `height` gets a vertex and
/// its unit offset direction; where it returns 0 the region stays attached. Returns the faces
/// that moved.
pub fn extrude_region(
  m: &mut LinkedMesh<()>,
  inside: impl Fn(&LinkedMesh<()>, FaceKey) -> bool,
  mut height: impl FnMut(&LinkedMesh<()>, VertexKey, Vec3) -> Result<f32, ErrorStack>,
) -> Result<FxHashSet<FaceKey>, ErrorStack> {
  let inside: FxHashSet<FaceKey> = m.faces.keys().filter(|&f| inside(m, f)).collect();
  let mut offsets: FxHashMap<VertexKey, Vec3> = FxHashMap::default();
  for group in coincident_groups(m) {
    let d = group_dir(m, &group, |f| inside.contains(&f));
    let Some(n) = d.try_normalize(1e-6) else {
      continue;
    };
    let mut h = 0.;
    for &v in &group {
      h += height(m, v, n)?;
    }
    h /= group.len() as f32;
    if h != 0. {
      offsets.extend(group.iter().map(|&v| (v, d * h)));
    }
  }

  // Contour edges as (inside face, index of the edge's first vertex in that face's winding),
  // which still addresses the edge once the face's corners are swapped for raised clones.
  let contour: Vec<(FaceKey, usize, [VertexKey; 2])> = m
    .edges
    .values()
    .filter(|e| e.faces.len() == 2 && inside.contains(&e.faces[0]) != inside.contains(&e.faces[1]))
    .map(|e| {
      let f = e.faces[usize::from(!inside.contains(&e.faces[0]))];
      let vs = m.faces[f].vertices;
      let i = (0..3)
        .find(|&i| e.vertices.contains(&vs[i]) && e.vertices.contains(&vs[(i + 1) % 3]))
        .unwrap();
      (f, i, [vs[i], vs[(i + 1) % 3]])
    })
    .collect();

  let keys: Vec<VertexKey> = m
    .vertices
    .keys()
    .filter(|k| offsets.contains_key(k))
    .collect();
  for v in keys {
    let off = offsets[&v];
    let fan = fan(m, v);
    let mut rest: SmallVec<[FaceKey; 12]> =
      fan.iter().copied().filter(|f| inside.contains(f)).collect();
    if rest.len() == fan.len() {
      m.vertices[v].position += off;
      continue;
    }
    // One clone per run of inside faces around the vertex: regions that only touch at a point
    // rise separately rather than pinching into a non-manifold vertex.
    while let Some(seed) = rest.pop() {
      let mut sector: SmallVec<[FaceKey; 12]> = SmallVec::from_slice(&[seed]);
      let mut i = 0;
      while i < sector.len() {
        for e in m.faces[sector[i]].edges {
          if !m.edges[e].vertices.contains(&v) {
            continue;
          }
          for &g in &m.edges[e].faces {
            if let Some(j) = rest.iter().position(|&r| r == g) {
              sector.push(rest.swap_remove(j));
            }
          }
        }
        i += 1;
      }
      let clone = m.split_off_faces(v, &sector);
      m.vertices[clone].position += off;
    }
  }

  for (f, i, [a, b]) in contour {
    let vs = m.faces[f].vertices;
    let (a2, b2) = (vs[i], vs[(i + 1) % 3]);
    if b2 != b {
      m.add_face::<false>([a, b, b2], ());
    }
    if a2 != a {
      m.add_face::<false>([a, b2, a2], ());
    }
  }
  Ok(inside)
}
