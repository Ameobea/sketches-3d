use std::rc::Rc;

use fxhash::FxHashMap;
use mesh::{
  bvh::TriBvh,
  linked_mesh::{Vec3, VertexKey},
  LinkedMesh,
};
use smallvec::SmallVec;

use crate::{
  mesh_ops::refine::{refine_field, Refine},
  ErrorStack, MeshHandle,
};

pub struct AoParams {
  pub samples: usize,
  pub max_dist: f32,
  /// Ray origins start this far along the normal so a vertex's own faces can't self-occlude.
  pub bias: f32,
  /// Exponent on the escaped fraction: open stays 1, partial occlusion deepens toward 0.
  pub intensity: f32,
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

pub(crate) struct Sampler<'a> {
  bvhs: Vec<Rc<TriBvh>>,
  set: Vec<[f32; 3]>,
  params: &'a AoParams,
}

impl<'a> Sampler<'a> {
  pub(crate) fn new(src: &MeshHandle, occluders: &[Rc<MeshHandle>], params: &'a AoParams) -> Self {
    Self {
      bvhs: std::iter::once(src)
        .chain(occluders.iter().map(|o| &**o))
        .map(|m| m.get_or_create_bvh())
        .collect(),
      set: hemisphere_set(params.samples),
      params,
    }
  }

  /// Fraction of cosine-weighted hemisphere rays from world-space `origin` around unit `n` that
  /// escape every BVH within `max_dist`; `seed` picks the sample set's rotation.
  pub(crate) fn ao(&self, seed: usize, origin: Vec3, n: Vec3) -> f32 {
    self.ao_rot(hash_rot(seed), origin, n)
  }

  /// Whether nothing rises above the tangent plane within range, i.e. every ray would escape.
  pub(crate) fn open_above(&self, origin: Vec3, n: Vec3) -> bool {
    let p = self.params;
    let origin = origin + n * p.bias;
    !self
      .bvhs
      .iter()
      .any(|bvh| bvh.any_above_plane(&origin, &n, p.max_dist))
  }

  /// Number of the sample set's rays at `idx` (rotated by `rot` about the normal) that escape.
  pub(crate) fn escaped(&self, rot: f32, origin: Vec3, n: Vec3, idx: &[usize]) -> u32 {
    let p = self.params;
    let origin = origin + n * p.bias;
    let (t, b) = tangent_frame(n);
    let (rs, rc) = rot.sin_cos();
    let dirs: SmallVec<[Vec3; 64]> = idx
      .iter()
      .map(|&i| {
        let [x, y, z] = self.set[i];
        t * (x * rc - y * rs) + b * (x * rs + y * rc) + n * z
      })
      .collect();
    dirs
      .chunks(64)
      .map(|chunk| {
        let all = if chunk.len() == 64 {
          u64::MAX
        } else {
          (1u64 << chunk.len()) - 1
        };
        self
          .bvhs
          .iter()
          .fold(all, |mask, bvh| {
            bvh.escaping_mask(&origin, chunk, p.max_dist, mask)
          })
          .count_ones()
      })
      .sum()
  }

  /// `rot` rotates the sample set about the normal.
  pub(crate) fn ao_rot(&self, rot: f32, origin: Vec3, n: Vec3) -> f32 {
    if self.open_above(origin, n) {
      return 1.;
    }
    let all: Vec<usize> = (0..self.params.samples).collect();
    self.shape(self.escaped(rot, origin, n, &all) as f32 / self.params.samples as f32)
  }

  pub(crate) fn shape(&self, ao: f32) -> f32 {
    match self.params.intensity {
      1. => ao,
      k => crate::dmath::powf(ao, k),
    }
  }
}

/// Bakes AO for every vertex of `out` (a clone of `src.mesh`): 1 = fully open, 0 = buried. With
/// `refine`, edges are bisected wherever linear interpolation would misrepresent the field;
/// midpoint splits leave the surface unchanged, so `src`'s BVH stays valid throughout.
pub fn bake_ao(
  src: &MeshHandle,
  out: &mut LinkedMesh<()>,
  occluders: &[Rc<MeshHandle>],
  params: &AoParams,
  refine: Option<&Refine>,
) -> Result<FxHashMap<VertexKey, f32>, ErrorStack> {
  let sampler = Sampler::new(src, occluders, params);

  let xf = src.transform;
  let linear = xf.fixed_view::<3, 3>(0, 0).into_owned();
  let normal_mat = linear
    .try_inverse()
    .map(|inv| inv.transpose())
    .unwrap_or(linear);
  let world = |p: Vec3| (xf * p.push(1.)).xyz();
  let world_n = |n: Vec3| {
    let w = normal_mat * n;
    let len = w.norm();
    if len > 1e-12 {
      w / len
    } else {
      Vec3::y()
    }
  };

  let keys: Vec<VertexKey> = out.vertices.keys().collect();
  let ix_of: FxHashMap<VertexKey, usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
  // Authored shading normals when complete; otherwise area-weighted face normals, which is all
  // the hemisphere orientation needs and avoids cloning the mesh for the crease-aware pass.
  let complete_shading =
    !out.shading_normals.is_empty() && out.shading_normals.len() == out.vertices.len();
  let mut normals: Vec<Vec3> = vec![Vec3::zeros(); keys.len()];
  if complete_shading {
    for (k, n) in out.shading_normals.iter() {
      normals[ix_of[&k]] = *n;
    }
  } else {
    for f in out.faces.values() {
      let [a, b, c] = f.vertices.map(|k| out.vertices[k].position);
      let n = (b - a).cross(&(c - a));
      for k in f.vertices {
        normals[ix_of[&k]] += n;
      }
    }
  }

  let ao: FxHashMap<VertexKey, f32> = keys
    .iter()
    .enumerate()
    .map(|(i, &k)| {
      (
        k,
        sampler.ao(i, world(out.vertices[k].position), world_n(normals[i])),
      )
    })
    .collect();

  let Some(r) = refine else { return Ok(ao) };
  let mut vals: FxHashMap<VertexKey, [f32; 4]> =
    ao.into_iter().map(|(k, v)| (k, [v, 0., 0., 0.])).collect();
  let mut seed = keys.len();
  refine_field(out, &xf, &mut vals, r, None, |m, p, site| {
    let n = site.normal(m);
    seed += 1;
    Ok([sampler.ao(seed, world(p), world_n(n)), 0., 0., 0.])
  })?;
  Ok(vals.into_iter().map(|(k, v)| (k, v[0])).collect())
}
