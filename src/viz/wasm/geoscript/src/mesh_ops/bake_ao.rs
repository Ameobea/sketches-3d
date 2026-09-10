use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{Vec3, VertexKey},
  occlusion::OcclusionBvh,
};
use smallvec::SmallVec;

use crate::{ErrorStack, MeshHandle};

pub struct AoParams {
  pub samples: usize,
  pub max_dist: f32,
  /// Ray origins start this far along the normal so a vertex's own faces can't self-occlude.
  pub bias: f32,
}

/// Cosine-weighted hemisphere sample set (Malley's method over a Hammersley sequence) in a local
/// frame: `(x, y, z)` with `z` along the normal. Rotated per vertex about `z` so neighbours don't
/// share one pattern.
fn hemisphere_set(samples: usize) -> Vec<[f32; 3]> {
  (0..samples)
    .map(|i| {
      let u = (i as f32 + 0.5) / samples as f32;
      let v = (i as u32).reverse_bits() as f32 / u32::MAX as f32;
      let phi = std::f32::consts::TAU * v;
      let r = u.sqrt();
      [r * phi.cos(), r * phi.sin(), (1. - u).sqrt()]
    })
    .collect()
}

fn tangent_frame(n: Vec3) -> (Vec3, Vec3) {
  let helper = if n.x.abs() > 0.9 {
    Vec3::y()
  } else {
    Vec3::x()
  };
  let t = n.cross(&helper).normalize();
  (t, n.cross(&t))
}

fn hash_rot(i: usize) -> f32 {
  let mut h = (i as u32).wrapping_mul(0x9E37_79B9);
  h ^= h >> 16;
  h = h.wrapping_mul(0x85EB_CA6B);
  h ^= h >> 13;
  h as f32 / u32::MAX as f32 * std::f32::consts::TAU
}

fn world_tris(m: &MeshHandle) -> Vec<[Vec3; 3]> {
  let xf = |v: Vec3| (m.transform * v.push(1.)).xyz();
  m.mesh
    .faces
    .values()
    .map(|f| f.vertices.map(|k| xf(m.mesh.vertices[k].position)))
    .collect()
}

/// Fraction of cosine-weighted hemisphere rays from each vertex that escape `mesh` and every
/// occluder within `max_dist`: 1 = fully open, 0 = buried.
pub fn bake_ao(
  mesh: &MeshHandle,
  occluders: &[Rc<MeshHandle>],
  params: &AoParams,
) -> Result<Vec<(VertexKey, f32)>, ErrorStack> {
  let mut bvhs: Vec<OcclusionBvh> = vec![OcclusionBvh::build(world_tris(mesh))];
  let set = hemisphere_set(params.samples);
  for o in occluders {
    bvhs.push(OcclusionBvh::build(world_tris(o)));
  }

  let linear = mesh.transform.fixed_view::<3, 3>(0, 0).into_owned();
  let normal_mat = linear
    .try_inverse()
    .map(|inv| inv.transpose())
    .unwrap_or(linear);
  let ix_of: FxHashMap<VertexKey, usize> = mesh
    .mesh
    .vertices
    .keys()
    .enumerate()
    .map(|(i, k)| (k, i))
    .collect();
  // Authored shading normals when complete; otherwise area-weighted face normals, which is all
  // the hemisphere orientation needs and avoids cloning the mesh for the crease-aware pass.
  let complete_shading = !mesh.mesh.shading_normals.is_empty()
    && mesh.mesh.shading_normals.len() == mesh.mesh.vertices.len();
  let mut normals: Vec<Vec3> = vec![Vec3::zeros(); ix_of.len()];
  if complete_shading {
    for (k, n) in mesh.mesh.shading_normals.iter() {
      normals[ix_of[&k]] = *n;
    }
  } else {
    for f in mesh.mesh.faces.values() {
      let [a, b, c] = f.vertices.map(|k| mesh.mesh.vertices[k].position);
      let n = (b - a).cross(&(c - a));
      for k in f.vertices {
        normals[ix_of[&k]] += n;
      }
    }
  }
  for n in &mut normals {
    let w = normal_mat * *n;
    let len = w.norm();
    *n = if len > 1e-12 { w / len } else { Vec3::y() };
  }

  let out = mesh
    .mesh
    .vertices
    .iter()
    .enumerate()
    .map(|(i, (k, vtx))| {
      let n = normals[i];
      let origin = (mesh.transform * vtx.position.push(1.)).xyz() + n * params.bias;
      // Nothing above the tangent plane within range means every hemisphere ray escapes.
      if !bvhs
        .iter()
        .any(|bvh| bvh.any_above_plane(&origin, &n, params.max_dist))
      {
        return (k, 1.);
      }
      let (t, b) = tangent_frame(n);
      let (rs, rc) = hash_rot(i).sin_cos();
      let dirs: SmallVec<[Vec3; 64]> = set
        .iter()
        .map(|[x, y, z]| t * (x * rc - y * rs) + b * (x * rs + y * rc) + n * *z)
        .collect();
      let open: u32 = dirs
        .chunks(64)
        .map(|chunk| {
          let all = if chunk.len() == 64 {
            u64::MAX
          } else {
            (1u64 << chunk.len()) - 1
          };
          bvhs
            .iter()
            .fold(all, |mask, bvh| {
              bvh.escaping_mask(&origin, chunk, params.max_dist, mask)
            })
            .count_ones()
        })
        .sum();
      (k, open as f32 / params.samples as f32)
    })
    .collect();
  Ok(out)
}
