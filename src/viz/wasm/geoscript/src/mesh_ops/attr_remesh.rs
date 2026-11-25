//! Attribute-aware plumbing for re-tessellating ops. Going in, attribute seams (and optionally the
//! borders of regions over which every attribute is affine) become feature edges the remesher must
//! keep. Coming out, attributes are sampled per face corner so seams survive as split vertices.
use fxhash::{FxHashMap, FxHashSet};
use mesh::linked_mesh::{mesh_flags, Mat4, Vec3, VertexKey};
use mesh::slotmap_utils::vkey;
use mesh::LinkedMesh;
use smallvec::SmallVec;

use super::mesh_ops::attr_budgets;
use crate::MeshHandle;

pub(crate) struct RemeshInput {
  pub verts: Vec<f32>,
  pub indices: Vec<u32>,
  /// Vertex index pairs into `verts`.
  pub features: Vec<u32>,
}

/// Unclamped barycentrics of `p` projected onto the triangle's plane.
fn plane_bary(p: &Vec3, [a, b, c]: &[Vec3; 3]) -> [f32; 3] {
  let (v0, v1, v2) = (b - a, c - a, p - a);
  let (d00, d01, d11) = (v0.dot(&v0), v0.dot(&v1), v1.dot(&v1));
  let (d20, d21) = (v2.dot(&v0), v2.dot(&v1));
  let denom = d00 * d11 - d01 * d01;
  let v = (d11 * d20 - d01 * d21) / denom;
  let w = (d00 * d21 - d01 * d20) / denom;
  [1. - v - w, v, w]
}

/// Positions + indices with coincident vertices welded (the remesher would see attribute-seam
/// duplicates as open boundaries). `affine = (attr_tol, dist_tol)` additionally grows face groups
/// whose attributes all follow the seed face's affine function within `attr_budgets`, and features
/// the edges between groups: retriangulating inside one group then loses nothing.
pub(crate) fn remesh_input(mesh: &LinkedMesh<()>, affine: Option<(f32, f32)>) -> RemeshInput {
  let mut ix_of = FxHashMap::default();
  let mut verts = Vec::with_capacity(mesh.vertices.len() * 3);
  let mut tris: Vec<([u32; 3], [VertexKey; 3])> = Vec::with_capacity(mesh.faces.len());
  for face in mesh.faces.values() {
    let ixs = face.vertices.map(|vk| {
      let p = mesh.vertices[vk].position;
      *ix_of
        .entry([p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
        .or_insert_with(|| {
          verts.extend_from_slice(p.as_slice());
          (verts.len() / 3 - 1) as u32
        })
    });
    if ixs[0] != ixs[1] && ixs[1] != ixs[2] && ixs[2] != ixs[0] {
      tris.push((ixs, face.vertices));
    }
  }
  let edge_key = |a: u32, b: u32| (a.min(b), a.max(b));
  let mut edges: FxHashMap<(u32, u32), SmallVec<[u32; 2]>> = FxHashMap::default();
  for (f, (ixs, _)) in tris.iter().enumerate() {
    for k in 0..3 {
      edges
        .entry(edge_key(ixs[k], ixs[(k + 1) % 3]))
        .or_default()
        .push(f as u32);
    }
  }
  if edges.values().any(|faces| faces.len() > 2) {
    // Coincident sheets that are distinct surfaces rather than one seam: leave them unwelded.
    let mut ix_of = FxHashMap::default();
    let mut verts = Vec::with_capacity(mesh.vertices.len() * 3);
    let indices = mesh
      .faces
      .values()
      .flat_map(|face| face.vertices)
      .map(|vk| {
        *ix_of.entry(vk).or_insert_with(|| {
          verts.extend_from_slice(mesh.vertices[vk].position.as_slice());
          (verts.len() / 3 - 1) as u32
        })
      })
      .collect();
    return RemeshInput {
      verts,
      indices,
      features: Vec::new(),
    };
  }

  let corner = |f: u32, ix: u32| {
    let (ixs, keys) = &tris[f as usize];
    keys[ixs.iter().position(|&i| i == ix).unwrap()]
  };
  let differs = |a: VertexKey, b: VertexKey| {
    a != b
      && mesh
        .vertex_channels
        .values()
        .any(|ch| ch.get(a) != ch.get(b))
  };
  let mut features: FxHashSet<(u32, u32)> = edges
    .iter()
    .filter(|(&(a, b), faces)| {
      faces.len() == 2
        && [a, b]
          .iter()
          .any(|&v| differs(corner(faces[0], v), corner(faces[1], v)))
    })
    .map(|(&e, _)| e)
    .collect();

  if let Some((attr_tol, dist_tol)) = affine {
    let budgets = attr_budgets(mesh, attr_tol, dist_tol);
    let pos = |f: u32| tris[f as usize].1.map(|vk| mesh.vertices[vk].position);
    let area = |f: u32| {
      let [a, b, c] = pos(f);
      (b - a).cross(&(c - a)).norm_squared()
    };
    // Large seeds first: their gradient estimate extrapolates best.
    let mut order: Vec<u32> = (0..tris.len() as u32).collect();
    order.sort_unstable_by(|&a, &b| area(b).total_cmp(&area(a)));
    let mut group = vec![u32::MAX; tris.len()];
    let mut queue = std::collections::VecDeque::new();
    for seed in order {
      if group[seed as usize] != u32::MAX || budgets.is_empty() {
        continue;
      }
      // Least-squares affine fit per lane over in-plane coordinates of the seed face, refit as
      // the group grows so one noisy seed doesn't set the gradient for the whole region.
      let [a, b, c] = pos(seed);
      let e1 = (b - a).normalize();
      let e2 = (c - a - e1 * (c - a).dot(&e1)).normalize();
      let row = |vk: VertexKey| {
        let d = mesh.vertices[vk].position - a;
        nalgebra::Vector3::new(d.dot(&e1), d.dot(&e2), 1.)
      };
      let lane_vals = |vk: VertexKey| {
        budgets.iter().flat_map(move |&(ch, lanes, _)| {
          let v = ch.get(vk).unwrap_or_default();
          (0..lanes).map(move |i| v[i])
        })
      };
      let lane_budgets: Vec<f32> = budgets
        .iter()
        .flat_map(|&(_, lanes, budget)| std::iter::repeat_n(budget, lanes))
        .collect();
      let mut ata = nalgebra::Matrix3::<f32>::zeros();
      let mut atb = vec![nalgebra::Vector3::<f32>::zeros(); lane_budgets.len()];
      let mut coeffs = atb.clone();
      let mut add = |f: u32, coeffs: &mut Vec<nalgebra::Vector3<f32>>| {
        for vk in tris[f as usize].1 {
          let r = row(vk);
          ata += r * r.transpose();
          for (atb, v) in atb.iter_mut().zip(lane_vals(vk)) {
            *atb += r * v;
          }
        }
        if let Some(inv) = ata.try_inverse() {
          for (c, atb) in coeffs.iter_mut().zip(&atb) {
            *c = inv * atb;
          }
        }
      };
      let fits = |f: u32, coeffs: &[nalgebra::Vector3<f32>]| {
        tris[f as usize].1.iter().all(|&vk| {
          let r = row(vk);
          lane_vals(vk)
            .zip(coeffs)
            .zip(&lane_budgets)
            .all(|((v, c), &budget)| (c.dot(&r) - v).abs() <= budget)
        })
      };
      add(seed, &mut coeffs);
      group[seed as usize] = seed;
      queue.push_back(seed);
      while let Some(f) = queue.pop_front() {
        let ixs = tris[f as usize].0;
        for k in 0..3 {
          let e = edge_key(ixs[k], ixs[(k + 1) % 3]);
          if features.contains(&e) {
            continue;
          }
          for &g in &edges[&e] {
            if group[g as usize] == u32::MAX && fits(g, &coeffs) {
              group[g as usize] = seed;
              queue.push_back(g);
              add(g, &mut coeffs);
            }
          }
        }
      }
    }
    features.extend(
      edges
        .iter()
        .filter(|(_, faces)| {
          faces.len() == 2 && group[faces[0] as usize] != group[faces[1] as usize]
        })
        .map(|(&e, _)| e),
    );
  }

  RemeshInput {
    verts,
    indices: tris.iter().flat_map(|(ixs, _)| *ixs).collect(),
    features: features.into_iter().flat_map(|(a, b)| [a, b]).collect(),
  }
}

/// Rebuilds remesher output `out` (local space, placed by `transform`) carrying `src`'s channels.
/// Each face corner is sampled just inside its own face, so a vertex sitting on a seam of `src`
/// gets one value per side and is split into duplicates; everywhere else this matches plain
/// nearest-point transfer.
pub(crate) fn transfer_corner_attrs(
  src: &MeshHandle,
  out: LinkedMesh<()>,
  transform: &Mat4,
  sharp_angle_rads: f32,
) -> LinkedMesh<()> {
  if src.mesh.vertex_channels.is_empty() || src.mesh.faces.is_empty() || out.faces.is_empty() {
    return out;
  }
  let bvh = src.get_or_create_bvh();
  let src_faces: Vec<_> = src.mesh.faces.values().map(|face| face.vertices).collect();
  let mut channels: Vec<_> = src.mesh.vertex_channels.iter().collect();
  channels.sort_unstable_by_key(|(name, _)| *name);
  let eps: Vec<f32> = channels
    .iter()
    .map(|(_, ch)| {
      let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
      for v in src.mesh.vertices.keys().filter_map(|v| ch.get(v)) {
        for &x in &v[..ch.store.arity()] {
          lo = lo.min(x);
          hi = hi.max(x);
        }
      }
      1e-3 * (hi - lo).max(lo.abs()).max(hi.abs())
    })
    .collect();
  let world = |p: Vec3| (transform * p.push(1.)).xyz();
  let sample = |tri: u32, bary: [f32; 3]| -> Vec<Option<[f32; 4]>> {
    let keys = src_faces[tri as usize];
    let srcs = [(keys[0], bary[0]), (keys[1], bary[1]), (keys[2], bary[2])];
    channels.iter().map(|(_, ch)| ch.blend(&srcs)).collect()
  };
  let same = |a: &[Option<[f32; 4]>], b: &[Option<[f32; 4]>]| {
    a.iter()
      .zip(b)
      .zip(&eps)
      .all(|((a, b), &eps)| match (a, b) {
        (Some(a), Some(b)) => (0..4).all(|i| (a[i] - b[i]).abs() <= eps),
        (None, None) => true,
        _ => false,
      })
  };

  let mut positions = Vec::with_capacity(out.vertices.len());
  let mut values: Vec<Vec<Option<[f32; 4]>>> = Vec::with_capacity(out.vertices.len());
  let mut copies: FxHashMap<VertexKey, SmallVec<[u32; 2]>> = FxHashMap::default();
  let mut indices = Vec::with_capacity(out.faces.len() * 3);
  let mut split = false;
  for face in out.faces.values() {
    let local = face.vertices.map(|vk| out.vertices[vk].position);
    let centroid = world((local[0] + local[1] + local[2]) / 3.);
    for (vk, local) in face.vertices.into_iter().zip(local) {
      let p = world(local);
      let (tri, bary, _) = bvh.closest_point(&p).unwrap();
      let mut value = sample(tri, bary);
      // The source triangle under this face, extrapolated back to the corner: differs from the
      // direct sample only when the corner is on a seam and the direct hit was the far side.
      let (tri, clamped, _) = bvh.closest_point(&(p + (centroid - p) * 0.1)).unwrap();
      let tri_pos = src_faces[tri as usize].map(|k| world(src.mesh.vertices[k].position));
      let bary = plane_bary(&p, &tri_pos);
      let inside = sample(
        tri,
        if bary.iter().all(|b| b.abs() <= 2.) {
          bary
        } else {
          clamped
        },
      );
      if !same(&value, &inside) {
        value = inside;
      }
      let slots = copies.entry(vk).or_default();
      let ix = match slots.iter().find(|&&ix| same(&values[ix as usize], &value)) {
        Some(&ix) => ix,
        None => {
          split |= !slots.is_empty();
          positions.push(local);
          values.push(value);
          slots.push(positions.len() as u32 - 1);
          positions.len() as u32 - 1
        }
      };
      indices.push(ix);
    }
  }

  let mut mesh = LinkedMesh::from_indexed_vertices(&positions, &indices, None, None);
  for (c, (name, ch)) in channels.into_iter().enumerate() {
    let mut dst = ch.empty_like();
    for (i, value) in values.iter().enumerate() {
      if let Some(v) = value[c] {
        dst.set(vkey(i as u32 + 1, 1), v);
      }
    }
    mesh.vertex_channels.insert(name.clone(), dst);
  }
  if split {
    // Seam duplicates must not be re-welded at render time, which in turn needs normals that
    // stay smooth across them.
    mesh.flags |= mesh_flags::NO_WELD;
    mesh.recompute_shading_normals_preserving_seams(sharp_angle_rads);
  }
  mesh
}

#[cfg(test)]
mod tests {
  use super::*;
  use mesh::linked_mesh::{Arity, Channel};

  /// A flat 2x1 strip of two quads whose UVs jump across the shared middle edge, plus a `color`
  /// that is linear over the left quad and kinked over the right one.
  fn seamed_strip() -> MeshHandle {
    let p = |x: f32, z: f32| Vec3::new(x, 0., z);
    let verts = [
      p(0., 0.),
      p(1., 0.),
      p(1., 1.),
      p(0., 1.),
      p(1., 0.),
      p(2., 0.),
      p(2., 1.),
      p(1., 1.),
      p(1.5, 0.5),
    ];
    let indices = [0, 2, 1, 0, 3, 2, 4, 8, 5, 5, 8, 6, 6, 8, 7, 7, 8, 4];
    let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
    mesh.flags |= mesh_flags::NO_WELD;
    let mut uv = Channel::for_attr("uv", Arity::Vec2);
    let mut color = Channel::for_attr("color", Arity::Vec3);
    for (i, v) in verts.iter().enumerate() {
      let key = vkey(i as u32 + 1, 1);
      let island = if i < 4 { 0. } else { 5. };
      uv.set(key, [v.x + island, v.z, 0., 0.]);
      let c = if i == 8 { 1. } else { v.x * 0.1 };
      color.set(key, [c, c, c, 0.]);
    }
    mesh.vertex_channels.insert("uv".to_owned(), uv);
    mesh.vertex_channels.insert("color".to_owned(), color);
    MeshHandle::new(std::rc::Rc::new(mesh))
  }

  #[test]
  fn seams_become_features_and_survive_retriangulation() {
    let src = seamed_strip();
    let seams_only = remesh_input(&src.mesh, None);
    assert_eq!(seams_only.verts.len() / 3, 7);
    assert_eq!(seams_only.features.len(), 2);

    // The color spike at the right quad's center keeps its fan apart from the left quad's faces.
    let affine = remesh_input(&src.mesh, Some((0.05, 0.01)));
    assert!(affine.features.len() > seams_only.features.len());

    // Retriangulate the welded strip with the other diagonals, dropping the center vertex.
    let p = |x: f32, z: f32| Vec3::new(x, 0., z);
    let out = LinkedMesh::from_indexed_vertices(
      &[
        p(0., 0.),
        p(1., 0.),
        p(2., 0.),
        p(2., 1.),
        p(1., 1.),
        p(0., 1.),
      ],
      &[0, 5, 1, 1, 5, 4, 1, 4, 2, 2, 4, 3],
      None,
      None,
    );
    let out = transfer_corner_attrs(&src, out, &Mat4::identity(), 0.8);
    assert!(out.has_flag(mesh_flags::NO_WELD));
    assert_eq!(out.vertices.len(), 8);
    let uv = &out.vertex_channels["uv"];
    for face in out.faces.values() {
      let us = face.vertices.map(|vk| uv.get(vk).unwrap()[0]);
      let xs = face.vertices.map(|vk| out.vertices[vk].position.x);
      let island = if xs.iter().sum::<f32>() > 3. { 5. } else { 0. };
      for (u, x) in us.iter().zip(xs) {
        assert!((u - (x + island)).abs() < 1e-4, "u={u} x={x}");
      }
    }
  }
}
