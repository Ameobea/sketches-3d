use std::{cell::Cell, rc::Rc};

use fxhash::FxHashMap;
use mesh::{
  bvh::TriBvh,
  linked_mesh::{DisplacementNormalMethod, EdgeKey, EdgeSplitPos, FaceKey, Vec3, VertexKey},
  LinkedMesh,
};
use smallvec::SmallVec;

use crate::{ErrorStack, MeshHandle};

pub struct AoParams {
  pub samples: usize,
  pub max_dist: f32,
  /// Ray origins start this far along the normal so a vertex's own faces can't self-occlude.
  pub bias: f32,
}

/// Adaptive refinement: while the AO sampled at an edge's midpoint differs from its endpoints'
/// average by more than `tol`, the containing faces are bisected (longest edge first), down to
/// edges of `min_edge` world units.
pub struct Refine {
  pub tol: f32,
  pub min_edge: f32,
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

struct Sampler<'a> {
  bvhs: Vec<Rc<TriBvh>>,
  set: Vec<[f32; 3]>,
  params: &'a AoParams,
}

impl Sampler<'_> {
  /// Fraction of cosine-weighted hemisphere rays from world-space `origin` around unit `n` that
  /// escape every BVH within `max_dist`; `seed` picks the sample set's rotation.
  fn ao(&self, seed: usize, origin: Vec3, n: Vec3) -> f32 {
    let p = self.params;
    let origin = origin + n * p.bias;
    // Nothing above the tangent plane within range means every hemisphere ray escapes.
    if !self
      .bvhs
      .iter()
      .any(|bvh| bvh.any_above_plane(&origin, &n, p.max_dist))
    {
      return 1.;
    }
    let (t, b) = tangent_frame(n);
    let (rs, rc) = hash_rot(seed).sin_cos();
    let dirs: SmallVec<[Vec3; 64]> = self
      .set
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
        self
          .bvhs
          .iter()
          .fold(all, |mask, bvh| {
            bvh.escaping_mask(&origin, chunk, p.max_dist, mask)
          })
          .count_ones()
      })
      .sum();
    open as f32 / p.samples as f32
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
  let sampler = Sampler {
    bvhs: std::iter::once(src)
      .chain(occluders.iter().map(|o| &**o))
      .map(|m| m.get_or_create_bvh())
      .collect(),
    set: hemisphere_set(params.samples),
    params,
  };

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

  let mut ao: FxHashMap<VertexKey, f32> = keys
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
  let pair = |a: VertexKey, b: VertexKey| if a < b { [a, b] } else { [b, a] };
  let seed = Cell::new(keys.len());
  let sample = |p: Vec3, n: Vec3| {
    seed.set(seed.get() + 1);
    sampler.ao(seed.get(), p, world_n(n))
  };
  let ends = |out: &LinkedMesh<()>, ek: EdgeKey| {
    out.edges[ek]
      .vertices
      .map(|k| world(out.vertices[k].position))
  };
  // Area-weighted over the edge's faces: the normal this point would get as a `tessellate`
  // vertex, rather than the endpoints' blend, which tilts into the surface beside creases.
  let edge_normal = |out: &LinkedMesh<()>, ek: EdgeKey| {
    out.edges[ek].faces.iter().fold(Vec3::zeros(), |acc, &f| {
      let [p, q, r] = out.faces[f].vertices.map(|k| out.vertices[k].position);
      acc + (q - p).cross(&(r - p))
    })
  };
  let edge_len = |out: &LinkedMesh<()>, ek: EdgeKey| {
    let [a, b] = ends(out, ek);
    (a - b).norm()
  };

  // Split seams leave every crease as coincident boundary edges, one per side. Those twins are
  // bisected together (midpoints kept bit-identical) so refinement can't T-junction along
  // creases. Found through coincident vertices, since vertices are never removed here while
  // boundary edges get re-keyed whenever a face on them splits.
  let pos_key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
  let mut coincident: FxHashMap<[u32; 3], SmallVec<[VertexKey; 3]>> = FxHashMap::default();
  for (k, v) in out.vertices.iter() {
    coincident.entry(pos_key(v.position)).or_default().push(k);
  }
  let group = |out: &LinkedMesh<()>,
               coincident: &FxHashMap<[u32; 3], SmallVec<[VertexKey; 3]>>,
               ek: EdgeKey|
   -> SmallVec<[EdgeKey; 2]> {
    let e = &out.edges[ek];
    if e.faces.len() != 1 {
      return std::iter::once(ek).collect();
    }
    let [c0, c1] = e
      .vertices
      .map(|k| &coincident[&pos_key(out.vertices[k].position)]);
    c0.iter()
      .flat_map(|&a| c1.iter().map(move |&b| [a, b]))
      .filter_map(|ab| out.get_edge_key(ab))
      .filter(|&t| out.edges[t].faces.len() == 1)
      .collect()
  };
  // Length, then the group's smallest key, so twins rank equal and one propagation walk follows
  // a strict order that can't cycle.
  let rank = |out: &LinkedMesh<()>, coincident: &FxHashMap<[u32; 3], _>, ek: EdgeKey| {
    (
      edge_len(out, ek),
      *group(out, coincident, ek).iter().min().unwrap(),
    )
  };
  let longest_edge = |out: &LinkedMesh<()>, coincident: &FxHashMap<[u32; 3], _>, fk: FaceKey| {
    out.faces[fk]
      .edges
      .iter()
      .copied()
      .max_by(|&a, &b| {
        let (la, ka) = rank(out, coincident, a);
        let (lb, kb) = rank(out, coincident, b);
        la.total_cmp(&lb).then(ka.cmp(&kb))
      })
      .unwrap()
  };
  // Sampled AO at edge midpoints / quarter points, keyed by endpoints so a split edge's quarter
  // samples become its halves' midpoints.
  let mut mid: FxHashMap<[VertexKey; 2], f32> = FxHashMap::default();
  let mut quarters: FxHashMap<[VertexKey; 2], [f32; 2]> = FxHashMap::default();

  let mut queue: Vec<FaceKey> = out.faces.keys().collect();
  while let Some(fk) = queue.pop() {
    let Some(face) = out.faces.get(fk) else {
      continue;
    };
    let longest = longest_edge(&out, &coincident, fk);
    if edge_len(&out, longest) < r.min_edge {
      continue;
    }
    let bad_edge = face.edges.iter().any(|&ek| {
      let [v0, v1] = out.edges[ek].vertices;
      let [a, b] = ends(&out, ek);
      if (a - b).norm() < r.min_edge {
        return false;
      }
      let (f0, f1) = (ao[&v0], ao[&v1]);
      let n = edge_normal(&out, ek);
      let m = *mid
        .entry(pair(v0, v1))
        .or_insert_with(|| sample((a + b) * 0.5, n));
      if (m - (f0 + f1) * 0.5).abs() > r.tol {
        return true;
      }
      // Steep edges get quarter probes too: an S-shaped transition centred on an edge puts its
      // midpoint right on the chord.
      (f0 - f1).abs() > r.tol && {
        let [q1, q3] = *quarters
          .entry(pair(v0, v1))
          .or_insert_with(|| [sample(a.lerp(&b, 0.25), n), sample(a.lerp(&b, 0.75), n)]);
        (q1 - (3. * f0 + f1) * 0.25).abs() > r.tol || (q3 - (f0 + 3. * f1) * 0.25).abs() > r.tol
      }
    });
    // Corners on creases carry compromise normals, so a big face can be wrong inside while every
    // edge midpoint agrees with its endpoints; the centroid catches that.
    let bad_centroid = || {
      let [p, q, r_] = face.vertices.map(|k| world(out.vertices[k].position));
      let mean = face.vertices.iter().map(|k| ao[k]).sum::<f32>() / 3.;
      (sample((p + q + r_) / 3., (q - p).cross(&(r_ - p))) - mean).abs() > r.tol
    };
    if !bad_edge && !bad_centroid() {
      continue;
    }
    // Rivara bisection: an edge is only split once it's the longest edge of every face on it
    // (twins included), walking to neighbours' longer edges first, which keeps the smallest
    // angle bounded.
    let mut stack = vec![longest];
    while let Some(&x) = stack.last() {
      let g = group(&out, &coincident, x);
      let blocker = g
        .iter()
        .flat_map(|&e| out.edges[e].faces.iter().copied())
        .map(|f| longest_edge(&out, &coincident, f))
        .find(|y| !g.contains(y));
      if let Some(y) = blocker {
        stack.push(y);
        continue;
      }
      let mut shared_mid = None;
      for &e in &g {
        let [v0, v1] = out.edges[e].vertices;
        let [a, b] = ends(&out, e);
        let n = edge_normal(&out, e);
        let m = *mid
          .entry(pair(v0, v1))
          .or_insert_with(|| sample((a + b) * 0.5, n));
        let vm = out.split_edge_cb(
          e,
          EdgeSplitPos::middle(),
          DisplacementNormalMethod::Interpolate,
          |_, _, _, new_faces| queue.extend(new_faces),
        );
        let p = *shared_mid.get_or_insert(out.vertices[vm].position);
        out.vertices[vm].position = p;
        coincident.entry(pos_key(p)).or_default().push(vm);
        ao.insert(vm, m);
        if let Some([q1, q3]) = quarters.remove(&pair(v0, v1)) {
          mid.insert(pair(v0, vm), q1);
          mid.insert(pair(vm, v1), q3);
        }
      }
      stack.pop();
    }
  }
  Ok(ao)
}
