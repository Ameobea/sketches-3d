//! Four-wide BVH for any-hit occlusion queries (AO, shadow rays): one slab test covers four
//! children, leaves hold a few triangles, and traversal exits on the first hit. Built by binned
//! SAH into a binary tree, then collapsed two levels at a time.

use smallvec::SmallVec;
use wide::{f32x4, CmpGe, CmpGt, CmpLe};

use crate::linked_mesh::Vec3;

const LEAF_TRIS: usize = 4;

pub struct OcclusionBvh {
  nodes: Vec<Node4>,
  tris: Vec<[Vec3; 3]>,
}

/// Children are SoA so the slab test vectorizes across the four lanes. `count > 0` marks a leaf
/// (`child` = first triangle); `count == 0 && child == u32::MAX` is an empty lane.
#[derive(Clone, Copy)]
struct Node4 {
  mins: [f32x4; 3],
  maxs: [f32x4; 3],
  child: [u32; 4],
  count: [u32; 4],
}

struct Prim {
  bmin: Vec3,
  bmax: Vec3,
  centroid: Vec3,
  ix: u32,
}

enum BinNode {
  Leaf {
    start: u32,
    count: u32,
    bmin: Vec3,
    bmax: Vec3,
  },
  Inner {
    left: u32,
    right: u32,
    bmin: Vec3,
    bmax: Vec3,
  },
}

impl BinNode {
  fn bounds(&self) -> (Vec3, Vec3) {
    match self {
      BinNode::Leaf { bmin, bmax, .. } | BinNode::Inner { bmin, bmax, .. } => (*bmin, *bmax),
    }
  }
}

fn area(bmin: Vec3, bmax: Vec3) -> f32 {
  let d = (bmax - bmin).map(|v| v.max(0.));
  2. * (d.x * d.y + d.y * d.z + d.z * d.x)
}

fn bounds_of(prims: &[Prim]) -> (Vec3, Vec3) {
  prims.iter().fold(
    (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY)),
    |(lo, hi), p| (lo.inf(&p.bmin), hi.sup(&p.bmax)),
  )
}

fn build_binary(prims: &mut [Prim], start: u32, out: &mut Vec<BinNode>) -> u32 {
  let (bmin, bmax) = bounds_of(prims);
  let n = prims.len();
  let leaf = |out: &mut Vec<BinNode>| {
    out.push(BinNode::Leaf {
      start,
      count: n as u32,
      bmin,
      bmax,
    });
    (out.len() - 1) as u32
  };
  if n <= LEAF_TRIS {
    return leaf(out);
  }

  let (cmin, cmax) = prims.iter().fold(
    (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY)),
    |(lo, hi), p| (lo.inf(&p.centroid), hi.sup(&p.centroid)),
  );
  let extent = cmax - cmin;
  let axis = extent.imax();
  let split = if extent[axis] <= 0. {
    n / 2
  } else {
    // binned SAH along the widest centroid axis
    const BINS: usize = 12;
    let scale = BINS as f32 / extent[axis];
    let bin_of = |p: &Prim| (((p.centroid[axis] - cmin[axis]) * scale) as usize).min(BINS - 1);
    let mut counts = [0usize; BINS];
    let mut bmins = [Vec3::repeat(f32::INFINITY); BINS];
    let mut bmaxs = [Vec3::repeat(f32::NEG_INFINITY); BINS];
    for p in prims.iter() {
      let b = bin_of(p);
      counts[b] += 1;
      bmins[b] = bmins[b].inf(&p.bmin);
      bmaxs[b] = bmaxs[b].sup(&p.bmax);
    }
    let mut best = (f32::INFINITY, 0usize);
    for cut in 1..BINS {
      let (mut lc, mut lmin, mut lmax) = (
        0,
        Vec3::repeat(f32::INFINITY),
        Vec3::repeat(f32::NEG_INFINITY),
      );
      for b in 0..cut {
        lc += counts[b];
        lmin = lmin.inf(&bmins[b]);
        lmax = lmax.sup(&bmaxs[b]);
      }
      let (mut rc, mut rmin, mut rmax) = (
        0,
        Vec3::repeat(f32::INFINITY),
        Vec3::repeat(f32::NEG_INFINITY),
      );
      for b in cut..BINS {
        rc += counts[b];
        rmin = rmin.inf(&bmins[b]);
        rmax = rmax.sup(&bmaxs[b]);
      }
      if lc == 0 || rc == 0 {
        continue;
      }
      let cost = area(lmin, lmax) * lc as f32 + area(rmin, rmax) * rc as f32;
      if cost < best.0 {
        best = (cost, cut);
      }
    }
    if best.1 == 0 {
      n / 2
    } else {
      let cut = best.1;
      let mut mid = 0;
      for i in 0..n {
        if bin_of(&prims[i]) < cut {
          prims.swap(i, mid);
          mid += 1;
        }
      }
      mid
    }
  };
  let split = split.clamp(1, n - 1);
  let (l, r) = prims.split_at_mut(split);
  let left = build_binary(l, start, out);
  let right = build_binary(r, start + split as u32, out);
  out.push(BinNode::Inner {
    left,
    right,
    bmin,
    bmax,
  });
  (out.len() - 1) as u32
}

impl OcclusionBvh {
  pub fn build(mut tris: Vec<[Vec3; 3]>) -> Self {
    let mut prims: Vec<Prim> = tris
      .iter()
      .enumerate()
      .map(|(ix, [a, b, c])| Prim {
        bmin: a.inf(b).inf(c),
        bmax: a.sup(b).sup(c),
        centroid: (a + b + c) / 3.,
        ix: ix as u32,
      })
      .collect();
    if prims.is_empty() {
      return Self {
        nodes: Vec::new(),
        tris,
      };
    }
    let mut bin = Vec::with_capacity(prims.len() / 2 + 1);
    let root = build_binary(&mut prims, 0, &mut bin);
    let order: Vec<u32> = prims.iter().map(|p| p.ix).collect();
    tris = order.iter().map(|&i| tris[i as usize]).collect();

    let mut nodes = Vec::with_capacity(bin.len() / 2 + 1);
    Self::collapse(&bin, root, &mut nodes);
    Self { nodes, tris }
  }

  /// Turns binary node `ix` into a four-wide node by greedily expanding the largest inner child
  /// until four lanes are used. Returns the node's index.
  fn collapse(bin: &[BinNode], ix: u32, nodes: &mut Vec<Node4>) -> u32 {
    let mut lanes: SmallVec<[u32; 4]> = match &bin[ix as usize] {
      BinNode::Leaf { .. } => SmallVec::from_slice(&[ix]),
      BinNode::Inner { left, right, .. } => SmallVec::from_slice(&[*left, *right]),
    };
    loop {
      if lanes.len() >= 4 {
        break;
      }
      let expandable = lanes
        .iter()
        .enumerate()
        .filter(|(_, &l)| matches!(bin[l as usize], BinNode::Inner { .. }))
        .max_by(|a, b| {
          let sa = |l: u32| {
            let (lo, hi) = bin[l as usize].bounds();
            area(lo, hi)
          };
          sa(*a.1).total_cmp(&sa(*b.1))
        })
        .map(|(i, _)| i);
      let Some(i) = expandable else { break };
      let BinNode::Inner { left, right, .. } = bin[lanes[i] as usize] else {
        unreachable!()
      };
      lanes[i] = left;
      lanes.push(right);
    }

    let slot = nodes.len();
    nodes.push(Node4 {
      mins: [f32x4::splat(f32::INFINITY); 3],
      maxs: [f32x4::splat(f32::NEG_INFINITY); 3],
      child: [u32::MAX; 4],
      count: [0; 4],
    });
    for (k, &l) in lanes.iter().enumerate() {
      let (lo, hi) = bin[l as usize].bounds();
      for axis in 0..3 {
        let mut mins = nodes[slot].mins[axis].to_array();
        let mut maxs = nodes[slot].maxs[axis].to_array();
        mins[k] = lo[axis];
        maxs[k] = hi[axis];
        nodes[slot].mins[axis] = f32x4::from(mins);
        nodes[slot].maxs[axis] = f32x4::from(maxs);
      }
      match bin[l as usize] {
        BinNode::Leaf { start, count, .. } => {
          nodes[slot].child[k] = start;
          nodes[slot].count[k] = count;
        }
        BinNode::Inner { .. } => {
          let child = Self::collapse(bin, l, nodes);
          nodes[slot].child[k] = child;
        }
      }
    }
    slot as u32
  }

  /// Whether the ray from `origin` along `dir` hits any triangle within `(0, max_t)`.
  pub fn any_hit(&self, origin: &Vec3, dir: &Vec3, max_t: f32) -> bool {
    if self.nodes.is_empty() {
      return false;
    }
    let o = [
      f32x4::splat(origin.x),
      f32x4::splat(origin.y),
      f32x4::splat(origin.z),
    ];
    let inv = [
      f32x4::splat(1. / dir.x),
      f32x4::splat(1. / dir.y),
      f32x4::splat(1. / dir.z),
    ];
    let mut stack = [0u32; 64];
    let mut sp = 1;
    while sp > 0 {
      sp -= 1;
      let node = &self.nodes[stack[sp] as usize];
      let hits = slab4(node, &o, &inv, max_t);
      for k in 0..4 {
        if hits & (1 << k) == 0 {
          continue;
        }
        if node.count[k] > 0 {
          let (start, count) = (node.child[k] as usize, node.count[k] as usize);
          if self.tris[start..start + count]
            .iter()
            .any(|t| ray_hits_triangle(origin, dir, t, max_t))
          {
            return true;
          }
        } else if node.child[k] != u32::MAX {
          stack[sp] = node.child[k];
          sp += 1;
        }
      }
    }
    false
  }

  /// Packet form of `any_hit` for up to 64 rays sharing `origin`: returns the subset of `active`
  /// whose rays reach `max_t` unhit. Rays traverse together so each node is loaded once per
  /// bundle and rays retire as soon as they hit.
  pub fn escaping_mask(&self, origin: &Vec3, dirs: &[Vec3], max_t: f32, active: u64) -> u64 {
    debug_assert!(dirs.len() <= 64);
    if self.nodes.is_empty() {
      return active;
    }
    let o = [
      f32x4::splat(origin.x),
      f32x4::splat(origin.y),
      f32x4::splat(origin.z),
    ];
    let inv: SmallVec<[[f32x4; 3]; 64]> = dirs
      .iter()
      .map(|d| {
        [
          f32x4::splat(1. / d.x),
          f32x4::splat(1. / d.y),
          f32x4::splat(1. / d.z),
        ]
      })
      .collect();
    let mut active = active;
    let mut stack = [(0u32, 0u64); 96];
    stack[0] = (0, active);
    let mut sp = 1;
    while sp > 0 && active != 0 {
      sp -= 1;
      let (ix, mask) = stack[sp];
      let mask = mask & active;
      if mask == 0 {
        continue;
      }
      let node = &self.nodes[ix as usize];
      let mut lane_masks = [0u64; 4];
      let mut m = mask;
      while m != 0 {
        let r = m.trailing_zeros() as usize;
        m &= m - 1;
        let hits = slab4(node, &o, &inv[r], max_t) as u64;
        for (k, lm) in lane_masks.iter_mut().enumerate() {
          *lm |= ((hits >> k) & 1) << r;
        }
      }
      for k in 0..4 {
        let lm = lane_masks[k] & active;
        if lm == 0 {
          continue;
        }
        if node.count[k] > 0 {
          let (start, count) = (node.child[k] as usize, node.count[k] as usize);
          let tris = &self.tris[start..start + count];
          let mut m = lm;
          while m != 0 {
            let r = m.trailing_zeros() as usize;
            m &= m - 1;
            if tris
              .iter()
              .any(|t| ray_hits_triangle(origin, &dirs[r], t, max_t))
            {
              active &= !(1u64 << r);
            }
          }
        } else if node.child[k] != u32::MAX {
          stack[sp] = (node.child[k], lm);
          sp += 1;
        }
      }
    }
    active
  }

  /// Whether any triangle within `max_dist` of `origin` rises above the plane through `origin`
  /// with normal `n`. When nothing does, no ray leaving `origin` into that hemisphere can hit
  /// anything, so callers can skip casting entirely.
  pub fn any_above_plane(&self, origin: &Vec3, n: &Vec3, max_dist: f32) -> bool {
    if self.nodes.is_empty() {
      return false;
    }
    let o = [
      f32x4::splat(origin.x),
      f32x4::splat(origin.y),
      f32x4::splat(origin.z),
    ];
    let nn = [f32x4::splat(n.x), f32x4::splat(n.y), f32x4::splat(n.z)];
    let max_sq = f32x4::splat(if max_dist == f32::MAX {
      f32::MAX
    } else {
      max_dist * max_dist
    });
    let zero = f32x4::splat(0.);
    let mut stack = [0u32; 64];
    let mut sp = 1;
    while sp > 0 {
      sp -= 1;
      let node = &self.nodes[stack[sp] as usize];
      // highest corner along n, and squared distance from origin to the box, per lane
      let (mut height, mut dist_sq) = (zero, zero);
      for axis in 0..3 {
        let (lo, hi) = (node.mins[axis] - o[axis], node.maxs[axis] - o[axis]);
        height += (lo * nn[axis]).fast_max(hi * nn[axis]);
        let d = lo.fast_max(zero).fast_max(-hi);
        dist_sq += d * d;
      }
      let live = (height.cmp_gt(zero) & dist_sq.cmp_le(max_sq)).move_mask();
      for k in 0..4 {
        if live & (1 << k) == 0 {
          continue;
        }
        if node.count[k] > 0 {
          let (start, count) = (node.child[k] as usize, node.count[k] as usize);
          if self.tris[start..start + count]
            .iter()
            .any(|t| t.iter().any(|v| (v - origin).dot(n) > 0.))
          {
            return true;
          }
        } else if node.child[k] != u32::MAX {
          stack[sp] = node.child[k];
          sp += 1;
        }
      }
    }
    false
  }
}

#[inline(always)]
fn slab4(node: &Node4, o: &[f32x4; 3], inv: &[f32x4; 3], max_t: f32) -> i32 {
  let mut tmin = f32x4::splat(0.);
  let mut tmax = f32x4::splat(max_t);
  for axis in 0..3 {
    let t1 = (node.mins[axis] - o[axis]) * inv[axis];
    let t2 = (node.maxs[axis] - o[axis]) * inv[axis];
    tmin = tmin.fast_max(t1.fast_min(t2));
    tmax = tmax.fast_min(t1.fast_max(t2));
  }
  tmax.cmp_ge(tmin).move_mask()
}

/// Möller–Trumbore, both faces, `(0, max_t)` exclusive.
#[inline(always)]
fn ray_hits_triangle(o: &Vec3, d: &Vec3, [a, b, c]: &[Vec3; 3], max_t: f32) -> bool {
  let (e1, e2) = (b - a, c - a);
  let pv = d.cross(&e2);
  let det = e1.dot(&pv);
  if det.abs() < 1e-12 {
    return false;
  }
  let inv = 1. / det;
  let s = o - a;
  let u = s.dot(&pv) * inv;
  if !(0. ..=1.).contains(&u) {
    return false;
  }
  let qv = s.cross(&e1);
  let v = d.dot(&qv) * inv;
  if v < 0. || u + v > 1. {
    return false;
  }
  let t = e2.dot(&qv) * inv;
  t > 0. && t < max_t
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn any_hit_matches_brute_force() {
    let mut seed = 12345u32;
    let mut rnd = || {
      seed ^= seed << 13;
      seed ^= seed >> 17;
      seed ^= seed << 5;
      (seed as f32 / u32::MAX as f32) * 2. - 1.
    };
    let tris: Vec<[Vec3; 3]> = (0..300)
      .map(|_| {
        let c = Vec3::new(rnd(), rnd(), rnd()) * 4.;
        [
          c + Vec3::new(rnd(), rnd(), rnd()),
          c + Vec3::new(rnd(), rnd(), rnd()),
          c + Vec3::new(rnd(), rnd(), rnd()),
        ]
      })
      .collect();
    let bvh = OcclusionBvh::build(tris.clone());
    for _ in 0..2000 {
      let o = Vec3::new(rnd(), rnd(), rnd()) * 5.;
      let d = Vec3::new(rnd(), rnd(), rnd()).normalize();
      let max_t = if rnd() > 0. { f32::MAX } else { 3. };
      let brute = tris.iter().any(|t| ray_hits_triangle(&o, &d, t, max_t));
      assert_eq!(bvh.any_hit(&o, &d, max_t), brute);
    }
    for _ in 0..200 {
      let o = Vec3::new(rnd(), rnd(), rnd()) * 5.;
      let dirs: Vec<Vec3> = (0..40)
        .map(|_| Vec3::new(rnd(), rnd(), rnd()).normalize())
        .collect();
      let expect = dirs.iter().enumerate().fold(0u64, |m, (i, d)| {
        m | ((!bvh.any_hit(&o, d, 3.)) as u64) << i
      });
      assert_eq!(bvh.escaping_mask(&o, &dirs, 3., (1u64 << 40) - 1), expect);
    }
  }
}
