//! Four-wide triangle BVH behind the mesh queries: any-hit rays (AO, `intersects_ray`), closest
//! point (attribute transfer) and tree-vs-tree overlap (`intersects`, self-intersection). Built
//! by binned SAH into a binary tree, then collapsed two levels at a time; one slab test covers
//! four children and leaves hold a few triangles.

use smallvec::SmallVec;
use wide::f32x4;

use crate::linked_mesh::Vec3;
use crate::triangle_intersection::tri_tri_intersection;

const LEAF_TRIS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
  pub mins: Vec3,
  pub maxs: Vec3,
}

impl Aabb {
  pub fn empty() -> Self {
    Self {
      mins: Vec3::repeat(f32::INFINITY),
      maxs: Vec3::repeat(f32::NEG_INFINITY),
    }
  }

  pub fn from_points(pts: impl IntoIterator<Item = Vec3>) -> Self {
    pts.into_iter().fold(Self::empty(), |b, p| Self {
      mins: b.mins.inf(&p),
      maxs: b.maxs.sup(&p),
    })
  }

  pub fn union(&self, o: &Self) -> Self {
    Self {
      mins: self.mins.inf(&o.mins),
      maxs: self.maxs.sup(&o.maxs),
    }
  }

  pub fn intersects(&self, o: &Self) -> bool {
    (0..3).all(|i| self.mins[i] <= o.maxs[i] && o.mins[i] <= self.maxs[i])
  }

  pub fn extent(&self) -> Vec3 {
    self.maxs - self.mins
  }
}

pub struct TriBvh {
  nodes: Vec<Node4>,
  tris: Vec<[Vec3; 3]>,
  /// input index of each (reordered) triangle
  prim: Vec<u32>,
}

/// Children are SoA so the slab test vectorizes across the four lanes. `count > 0` marks a leaf
/// (`child` = first triangle); `count == 0 && child == u32::MAX` is an empty lane, whose
/// inverted bounds fail every overlap and distance test on their own. Bounds are padded by a few
/// ulps so rays and boxes sitting exactly on a face stay candidates; leaf tests decide exactly.
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

type Bounds = (Vec3, Vec3);

fn empty() -> Bounds {
  (Vec3::repeat(f32::INFINITY), Vec3::repeat(f32::NEG_INFINITY))
}

#[inline(always)]
fn grow(b: Bounds, lo: &Vec3, hi: &Vec3) -> Bounds {
  (b.0.inf(lo), b.1.sup(hi))
}

fn area((bmin, bmax): Bounds) -> f32 {
  let d = (bmax - bmin).map(|v| v.max(0.));
  2. * (d.x * d.y + d.y * d.z + d.z * d.x)
}

fn bounds_of(prims: &[Prim]) -> Bounds {
  prims.iter().fold(empty(), |b, p| grow(b, &p.bmin, &p.bmax))
}

fn centroid_bounds(prims: &[Prim]) -> Bounds {
  prims
    .iter()
    .fold(empty(), |b, p| grow(b, &p.centroid, &p.centroid))
}

/// Box and centroid bounds come from the parent so each level costs one binning pass and one
/// partition pass.
fn build_binary(
  prims: &mut [Prim],
  start: u32,
  (bmin, bmax): Bounds,
  (cmin, cmax): Bounds,
  out: &mut Vec<BinNode>,
) -> u32 {
  let n = prims.len();
  if n <= LEAF_TRIS {
    out.push(BinNode::Leaf {
      start,
      count: n as u32,
      bmin,
      bmax,
    });
    return (out.len() - 1) as u32;
  }

  let extent = cmax - cmin;
  let axis = extent.imax();
  let (split, lb, lc, rb, rc) = 'sah: {
    if extent[axis] > 0. {
      // binned SAH along the widest centroid axis; the bins' unions give the children's bounds
      const BINS: usize = 12;
      let scale = BINS as f32 / extent[axis];
      let bin_of = |p: &Prim| (((p.centroid[axis] - cmin[axis]) * scale) as usize).min(BINS - 1);
      let mut counts = [0u32; BINS];
      let mut bins = [empty(); BINS];
      for p in prims.iter() {
        let b = bin_of(p);
        counts[b] += 1;
        bins[b] = grow(bins[b], &p.bmin, &p.bmax);
      }
      let mut right = [(0u32, empty()); BINS];
      let mut acc = (0u32, empty());
      for cut in (1..BINS).rev() {
        acc = (acc.0 + counts[cut], grow(acc.1, &bins[cut].0, &bins[cut].1));
        right[cut] = acc;
      }
      let mut best = (f32::INFINITY, 0usize, empty(), empty());
      let mut left = (0u32, empty());
      for cut in 1..BINS {
        left = (
          left.0 + counts[cut - 1],
          grow(left.1, &bins[cut - 1].0, &bins[cut - 1].1),
        );
        let r = right[cut];
        if left.0 == 0 || r.0 == 0 {
          continue;
        }
        let cost = area(left.1) * left.0 as f32 + area(r.1) * r.0 as f32;
        if cost < best.0 {
          best = (cost, cut, left.1, r.1);
        }
      }
      if best.1 > 0 {
        let cut = best.1;
        let (mut mid, mut lc, mut rc) = (0, empty(), empty());
        for i in 0..n {
          let c = prims[i].centroid;
          if bin_of(&prims[i]) < cut {
            lc = grow(lc, &c, &c);
            prims.swap(i, mid);
            mid += 1;
          } else {
            rc = grow(rc, &c, &c);
          }
        }
        break 'sah (mid, best.2, lc, best.3, rc);
      }
    }
    let mid = n / 2;
    let (l, r) = prims.split_at(mid);
    (
      mid,
      bounds_of(l),
      centroid_bounds(l),
      bounds_of(r),
      centroid_bounds(r),
    )
  };
  let (l, r) = prims.split_at_mut(split);
  let left = build_binary(l, start, lb, lc, out);
  let right = build_binary(r, start + split as u32, rb, rc, out);
  out.push(BinNode::Inner {
    left,
    right,
    bmin,
    bmax,
  });
  (out.len() - 1) as u32
}

/// Lane reference: a leaf's triangle run or an inner child, addressed through its parent node.
type Cell = (u32, u8);

impl TriBvh {
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
        prim: Vec::new(),
      };
    }
    let mut bin = Vec::with_capacity(prims.len() / 2 + 1);
    let (b, c) = (bounds_of(&prims), centroid_bounds(&prims));
    let root = build_binary(&mut prims, 0, b, c, &mut bin);
    let prim: Vec<u32> = prims.iter().map(|p| p.ix).collect();
    tris = prim.iter().map(|&i| tris[i as usize]).collect();

    let mut nodes = Vec::with_capacity(bin.len() / 2 + 1);
    Self::collapse(&bin, root, &mut nodes);
    Self { nodes, tris, prim }
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
          let sa = |l: u32| area(bin[l as usize].bounds());
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
        mins[k] = lo[axis] - pad(lo[axis]);
        maxs[k] = hi[axis] + pad(hi[axis]);
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
    let inv = inv_dir(dir);
    let mut stack: SmallVec<[u32; 64]> = SmallVec::new();
    stack.push(0);
    while let Some(ix) = stack.pop() {
      let node = &self.nodes[ix as usize];
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
          stack.push(node.child[k]);
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
    let inv: SmallVec<[[f32x4; 3]; 64]> = dirs.iter().map(inv_dir).collect();
    let mut active = active;
    let mut stack: SmallVec<[(u32, u64); 96]> = SmallVec::new();
    stack.push((0, active));
    while active != 0 {
      let Some((ix, mask)) = stack.pop() else { break };
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
          stack.push((node.child[k], lm));
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
    let mut stack: SmallVec<[u32; 64]> = SmallVec::new();
    stack.push(0);
    while let Some(ix) = stack.pop() {
      let node = &self.nodes[ix as usize];
      // highest corner along n, and squared distance from origin to the box, per lane
      let (mut height, mut dist_sq) = (zero, zero);
      for axis in 0..3 {
        let (lo, hi) = (node.mins[axis] - o[axis], node.maxs[axis] - o[axis]);
        height += (lo * nn[axis]).fast_max(hi * nn[axis]);
        let d = lo.fast_max(zero).fast_max(-hi);
        dist_sq += d * d;
      }
      let live = (height.simd_gt(zero) & dist_sq.simd_le(max_sq)).to_bitmask();
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
          stack.push(node.child[k]);
        }
      }
    }
    false
  }

  /// Closest surface point to `p`: `(input triangle index, barycentric coords, squared distance)`.
  pub fn closest_point(&self, p: &Vec3) -> Option<(u32, [f32; 3], f32)> {
    if self.nodes.is_empty() {
      return None;
    }
    let o = [f32x4::splat(p.x), f32x4::splat(p.y), f32x4::splat(p.z)];
    let zero = f32x4::splat(0.);
    let mut best = (0u32, [1., 0., 0.], f32::INFINITY);
    let mut stack: SmallVec<[(u32, f32); 64]> = SmallVec::new();
    stack.push((0, 0.));
    while let Some((ix, bound)) = stack.pop() {
      if bound >= best.2 {
        continue;
      }
      let node = &self.nodes[ix as usize];
      let mut dist_sq = zero;
      for axis in 0..3 {
        let d = (node.mins[axis] - o[axis])
          .fast_max(o[axis] - node.maxs[axis])
          .fast_max(zero);
        dist_sq += d * d;
      }
      let d = dist_sq.to_array();
      let mut order = [0usize, 1, 2, 3];
      order.sort_unstable_by(|&a, &b| d[a].total_cmp(&d[b]));
      // nearest leaves first so `best` tightens before the rest are tested
      for &k in &order {
        if node.count[k] == 0 || d[k] >= best.2 {
          continue;
        }
        let (start, count) = (node.child[k] as usize, node.count[k] as usize);
        for (i, t) in self.tris[start..start + count].iter().enumerate() {
          let (bary, q) = closest_point_on_triangle(p, t);
          let dd = (q - p).norm_squared();
          if dd < best.2 {
            best = (self.prim[start + i], bary, dd);
          }
        }
      }
      // inner lanes pushed farthest first so the nearest pops next
      for &k in order.iter().rev() {
        if node.count[k] == 0 && node.child[k] != u32::MAX && d[k] < best.2 {
          stack.push((node.child[k], d[k]));
        }
      }
    }
    (best.2 < f32::INFINITY).then_some(best)
  }

  /// Visits every triangle pair whose bounds overlap (padded by `eps`), descending both trees
  /// together and testing one lane against four at a time; `cb` gets input indices with positions
  /// and returns true to stop. With `other` being `self`, each unordered pair is visited once.
  pub fn for_each_pair(
    &self,
    other: &Self,
    eps: f32,
    mut cb: impl FnMut(u32, &[Vec3; 3], u32, &[Vec3; 3]) -> bool,
  ) {
    if self.nodes.is_empty() || other.nodes.is_empty() {
      return;
    }
    let same = std::ptr::eq(self, other);
    let e = f32x4::splat(eps);
    // lanes of `nb` overlapping lane `ja` of `na`
    let overlap = |na: &Node4, ja: usize, nb: &Node4| -> u32 {
      let mut m = 0b1111u32;
      for ax in 0..3 {
        let lo = f32x4::splat(na.mins[ax].as_array()[ja]);
        let hi = f32x4::splat(na.maxs[ax].as_array()[ja]);
        m &= ((nb.maxs[ax] + e).simd_ge(lo) & (nb.mins[ax] - e).simd_le(hi)).to_bitmask();
      }
      m
    };
    let push_pairs = |stack: &mut SmallVec<[(Cell, Cell); 64]>, ia: u32, ib: u32| {
      let (na, nb) = (&self.nodes[ia as usize], &other.nodes[ib as usize]);
      for j in 0..4 {
        let mut m = overlap(na, j, nb);
        if same && ia == ib {
          m &= !((1u32 << j) - 1);
        }
        while m != 0 {
          let k = m.trailing_zeros();
          m &= m - 1;
          stack.push(((ia, j as u8), (ib, k as u8)));
        }
      }
    };

    let mut stack: SmallVec<[(Cell, Cell); 64]> = SmallVec::new();
    push_pairs(&mut stack, 0, 0);
    while let Some(((ia, ja), (ib, kb))) = stack.pop() {
      let (na, nb) = (&self.nodes[ia as usize], &other.nodes[ib as usize]);
      let (ja, kb) = (ja as usize, kb as usize);
      match (na.count[ja] > 0, nb.count[kb] > 0) {
        (true, true) => {
          let (sa, ca) = (na.child[ja] as usize, na.count[ja] as usize);
          let (sb, cb_) = (nb.child[kb] as usize, nb.count[kb] as usize);
          let same_leaf = same && ia == ib && ja == kb;
          for a in sa..sa + ca {
            for b in sb..sb + cb_ {
              if same_leaf && b <= a {
                continue;
              }
              let (ta, tb) = (&self.tris[a], &other.tris[b]);
              if tri_bounds_overlap(ta, tb, eps) && cb(self.prim[a], ta, other.prim[b], tb) {
                return;
              }
            }
          }
        }
        (true, false) => {
          let ib2 = nb.child[kb];
          let mut m = overlap(na, ja, &other.nodes[ib2 as usize]);
          while m != 0 {
            let k = m.trailing_zeros();
            m &= m - 1;
            stack.push(((ia, ja as u8), (ib2, k as u8)));
          }
        }
        (false, true) => {
          let ia2 = na.child[ja];
          let mut m = overlap(nb, kb, &self.nodes[ia2 as usize]);
          while m != 0 {
            let j = m.trailing_zeros();
            m &= m - 1;
            stack.push(((ia2, j as u8), (ib, kb as u8)));
          }
        }
        (false, false) => push_pairs(&mut stack, na.child[ja], nb.child[kb]),
      }
    }
  }

  /// Whether any triangle of `self` intersects one of `other` (surfaces only; containment and
  /// point/edge-only contact don't count).
  pub fn intersects(&self, other: &Self) -> bool {
    let mut hit = false;
    self.for_each_pair(other, 0., |_, a, _, b| {
      hit = tri_tri_intersection(a[0], a[1], a[2], b[0], b[1], b[2]).is_some();
      hit
    });
    hit
  }
}

#[inline(always)]
fn pad(v: f32) -> f32 {
  v.abs() * 1e-6 + 1e-7
}

/// Keeps the inverse finite: an infinite one makes the slab test produce `0 * inf = NaN` for
/// origins on a box face, and NaN min/max semantics differ between SSE and wasm.
#[inline(always)]
fn inv_dir(d: &Vec3) -> [f32x4; 3] {
  let inv = |x: f32| f32x4::splat(1. / if x.abs() < 1e-30 { 1e-30 } else { x });
  [inv(d.x), inv(d.y), inv(d.z)]
}

#[inline(always)]
fn slab4(node: &Node4, o: &[f32x4; 3], inv: &[f32x4; 3], max_t: f32) -> u32 {
  let mut tmin = f32x4::splat(0.);
  let mut tmax = f32x4::splat(max_t);
  for axis in 0..3 {
    let t1 = (node.mins[axis] - o[axis]) * inv[axis];
    let t2 = (node.maxs[axis] - o[axis]) * inv[axis];
    tmin = tmin.fast_max(t1.fast_min(t2));
    tmax = tmax.fast_min(t1.fast_max(t2));
  }
  tmax.simd_ge(tmin).to_bitmask()
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

#[inline(always)]
fn tri_bounds_overlap(a: &[Vec3; 3], b: &[Vec3; 3], eps: f32) -> bool {
  (0..3).all(|ax| {
    let (alo, ahi) = (
      a[0][ax].min(a[1][ax]).min(a[2][ax]),
      a[0][ax].max(a[1][ax]).max(a[2][ax]),
    );
    let (blo, bhi) = (
      b[0][ax].min(b[1][ax]).min(b[2][ax]),
      b[0][ax].max(b[1][ax]).max(b[2][ax]),
    );
    alo - eps <= bhi && blo - eps <= ahi
  })
}

/// Ericson, *Real-Time Collision Detection* §5.1.5. Returns barycentrics and the point.
fn closest_point_on_triangle(p: &Vec3, [a, b, c]: &[Vec3; 3]) -> ([f32; 3], Vec3) {
  let (ab, ac, ap) = (b - a, c - a, p - a);
  let (d1, d2) = (ab.dot(&ap), ac.dot(&ap));
  if d1 <= 0. && d2 <= 0. {
    return ([1., 0., 0.], *a);
  }
  let bp = p - b;
  let (d3, d4) = (ab.dot(&bp), ac.dot(&bp));
  if d3 >= 0. && d4 <= d3 {
    return ([0., 1., 0.], *b);
  }
  let vc = d1 * d4 - d3 * d2;
  if vc <= 0. && d1 >= 0. && d3 <= 0. {
    let v = d1 / (d1 - d3);
    return ([1. - v, v, 0.], a + ab * v);
  }
  let cp = p - c;
  let (d5, d6) = (ab.dot(&cp), ac.dot(&cp));
  if d6 >= 0. && d5 <= d6 {
    return ([0., 0., 1.], *c);
  }
  let vb = d5 * d2 - d1 * d6;
  if vb <= 0. && d2 >= 0. && d6 <= 0. {
    let w = d2 / (d2 - d6);
    return ([1. - w, 0., w], a + ac * w);
  }
  let va = d3 * d6 - d5 * d4;
  if va <= 0. && d4 - d3 >= 0. && d5 - d6 >= 0. {
    let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
    return ([0., 1. - w, w], b + (c - b) * w);
  }
  let denom = 1. / (va + vb + vc);
  let (v, w) = (vb * denom, vc * denom);
  ([1. - v - w, v, w], a + ab * v + ac * w)
}

#[cfg(test)]
mod tests {
  use std::collections::HashSet;

  use super::*;

  fn rng(seed: u32) -> impl FnMut() -> f32 {
    let mut s = seed;
    move || {
      s ^= s << 13;
      s ^= s >> 17;
      s ^= s << 5;
      (s as f32 / u32::MAX as f32) * 2. - 1.
    }
  }

  fn soup(rnd: &mut impl FnMut() -> f32, n: usize, spread: f32) -> Vec<[Vec3; 3]> {
    (0..n)
      .map(|_| {
        let c = Vec3::new(rnd(), rnd(), rnd()) * spread;
        [
          c + Vec3::new(rnd(), rnd(), rnd()),
          c + Vec3::new(rnd(), rnd(), rnd()),
          c + Vec3::new(rnd(), rnd(), rnd()),
        ]
      })
      .collect()
  }

  fn unit_box() -> Vec<[Vec3; 3]> {
    let h = 0.5;
    let v = |x: f32, y: f32, z: f32| Vec3::new(x, y, z);
    let quad = |a: Vec3, b: Vec3, c: Vec3, d: Vec3| [[a, b, c], [a, c, d]];
    [
      quad(v(-h, -h, -h), v(h, -h, -h), v(h, h, -h), v(-h, h, -h)),
      quad(v(-h, -h, h), v(h, -h, h), v(h, h, h), v(-h, h, h)),
      quad(v(-h, -h, -h), v(-h, h, -h), v(-h, h, h), v(-h, -h, h)),
      quad(v(h, -h, -h), v(h, h, -h), v(h, h, h), v(h, -h, h)),
      quad(v(-h, -h, -h), v(h, -h, -h), v(h, -h, h), v(-h, -h, h)),
      quad(v(-h, h, -h), v(h, h, -h), v(h, h, h), v(-h, h, h)),
    ]
    .concat()
  }

  #[test]
  fn any_hit_matches_brute_force() {
    let mut rnd = rng(12345);
    let tris = soup(&mut rnd, 300, 4.);
    let bvh = TriBvh::build(tris.clone());
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

  #[test]
  fn axis_rays_on_box_planes() {
    let tris = unit_box();
    let bvh = TriBvh::build(tris.clone());
    let v = Vec3::new;
    let cases = [
      (v(0., 0., -5.), v(0., 0., 1.)),
      (v(0.5, 0.5, -5.), v(0., 0., 1.)),
      (v(0.5, 0., -5.), v(0., 0., 1.)),
      (v(-0.5, 0., -5.), v(0., 0., 1.)),
      (v(0., -0.5, -5.), v(0., 0., 1.)),
      (v(0., 0., -0.5), v(0., 0., 1.)),
      (v(0., 0., -0.5), v(0., 0., -1.)),
      (v(0.5, 0., 5.), v(0., 0., -1.)),
      (v(-5., 0.5, 0.), v(1., 0., 0.)),
      (v(0.5, 0., -5.), v(-0., -0., 1.)),
      (v(0.6, 0., -5.), v(0., 0., 1.)),
      (v(0.5001, 0., -5.), v(0., 0., 1.)),
      (v(-5., 0.5, 0.5), v(1., 0., 0.)),
    ];
    for (o, d) in cases {
      let brute = tris.iter().any(|t| ray_hits_triangle(&o, &d, t, f32::MAX));
      assert_eq!(bvh.any_hit(&o, &d, f32::MAX), brute, "{o:?} {d:?}");
      assert_eq!(
        bvh.escaping_mask(&o, &[d], f32::MAX, 1) == 0,
        brute,
        "packet {o:?} {d:?}"
      );
    }
  }

  #[test]
  fn closest_point_matches_brute_force() {
    let mut rnd = rng(777);
    let tris = soup(&mut rnd, 400, 4.);
    let bvh = TriBvh::build(tris.clone());
    for _ in 0..500 {
      let p = Vec3::new(rnd(), rnd(), rnd()) * 6.;
      let brute = tris
        .iter()
        .map(|t| (closest_point_on_triangle(&p, t).1 - p).norm_squared())
        .fold(f32::INFINITY, f32::min);
      let (ix, bary, dd) = bvh.closest_point(&p).unwrap();
      assert!((dd - brute).abs() <= 1e-4 * (1. + brute), "{dd} vs {brute}");
      let [a, b, c] = tris[ix as usize];
      let q = a * bary[0] + b * bary[1] + c * bary[2];
      assert!(((q - p).norm_squared() - dd).abs() <= 1e-3 * (1. + dd));
    }
    let bx = TriBvh::build(unit_box());
    let (_, bary, dd) = bx.closest_point(&Vec3::new(0.1, 0.2, 2.)).unwrap();
    assert!((dd - 2.25).abs() < 1e-6 && (bary.iter().sum::<f32>() - 1.).abs() < 1e-6);
    assert!(TriBvh::build(Vec::new())
      .closest_point(&Vec3::zeros())
      .is_none());
  }

  #[test]
  fn pairs_match_brute_force() {
    let mut rnd = rng(4242);
    let (ta, tb) = (soup(&mut rnd, 200, 3.), soup(&mut rnd, 150, 3.));
    let (ba, bb) = (TriBvh::build(ta.clone()), TriBvh::build(tb.clone()));
    let boxes = |ts: &[[Vec3; 3]]| -> Vec<Aabb> {
      ts.iter()
        .map(|t| Aabb::from_points(t.iter().copied()))
        .collect()
    };
    let (xa, xb) = (boxes(&ta), boxes(&tb));

    let mut got = Vec::new();
    ba.for_each_pair(&bb, 0., |a, pa, b, pb| {
      assert_eq!((*pa, *pb), (ta[a as usize], tb[b as usize]));
      got.push((a, b));
      false
    });
    let want: HashSet<(u32, u32)> = (0..ta.len())
      .flat_map(|i| (0..tb.len()).map(move |j| (i as u32, j as u32)))
      .filter(|&(i, j)| xa[i as usize].intersects(&xb[j as usize]))
      .collect();
    assert_eq!(got.len(), want.len());
    assert_eq!(got.into_iter().collect::<HashSet<_>>(), want);

    let mut got = Vec::new();
    ba.for_each_pair(&ba, 1e-6, |a, _, b, _| {
      got.push((a.min(b), a.max(b)));
      false
    });
    let want: HashSet<(u32, u32)> = (0..ta.len())
      .flat_map(|i| (i + 1..ta.len()).map(move |j| (i as u32, j as u32)))
      .filter(|&(i, j)| xa[i as usize].intersects(&xa[j as usize]))
      .collect();
    assert_eq!(got.len(), want.len());
    assert_eq!(got.into_iter().collect::<HashSet<_>>(), want);

    let brute = ta.iter().any(|a| {
      tb.iter()
        .any(|b| tri_tri_intersection(a[0], a[1], a[2], b[0], b[1], b[2]).is_some())
    });
    assert!(brute && ba.intersects(&bb));
    let far = TriBvh::build(tb.iter().map(|t| t.map(|v| v + Vec3::x() * 100.)).collect());
    assert!(!ba.intersects(&far) && !ba.intersects(&TriBvh::build(Vec::new())));

    let mut stopped = 0;
    ba.for_each_pair(&bb, 0., |_, _, _, _| {
      stopped += 1;
      true
    });
    assert_eq!(stopped, 1);
  }
}
