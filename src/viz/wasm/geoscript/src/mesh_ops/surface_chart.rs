//! Local 2D coordinates on a mesh, grown outward from a seed point: a discrete exponential map
//! (Schmidt, Grimm & Wyvill 2006, "Interactive Decal Compositing with Discrete Exponential
//! Maps"). Vertices settle in order of edge distance from the seed, each placed from its
//! already-settled neighbours. Exact on developable regions, increasingly distorted with the
//! curvature enclosed between a vertex and the seed.

use std::{cmp::Reverse, collections::BinaryHeap};

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{Vec3, VertexKey},
  LinkedMesh,
};
use nalgebra::{Rotation3, Vector2};

use super::displace::{coincident_groups, group_dir};

/// Coordinate given to vertices the seed's component doesn't reach.
pub const UNREACHED: f32 = 1e9;

/// `origin` maps to (0, 0), `up` (projected onto the surface there) to +y, and +x lies to its
/// right seen from outside.
pub fn surface_chart(
  m: &LinkedMesh<()>,
  origin: Vec3,
  up: Vec3,
) -> FxHashMap<VertexKey, Vector2<f32>> {
  // Seam twins are one surface point.
  let groups = coincident_groups(m);
  let group_of: FxHashMap<VertexKey, usize> = groups
    .iter()
    .enumerate()
    .flat_map(|(i, g)| g.iter().map(move |&k| (k, i)))
    .collect();
  let pos: Vec<Vec3> = groups.iter().map(|g| m.vertices[g[0]].position).collect();
  let normal: Vec<Vec3> = groups
    .iter()
    .map(|g| {
      group_dir(m, g, |_| true)
        .try_normalize(1e-6)
        .unwrap_or_else(Vec3::y)
    })
    .collect();
  let mut uv = vec![Vector2::repeat(UNREACHED); groups.len()];
  let nearness = |i: &usize| (pos[*i] - origin).norm_squared();
  let Some(seed) = (0..groups.len()).min_by(|a, b| nearness(a).total_cmp(&nearness(b))) else {
    return FxHashMap::default();
  };

  let flat = |v: Vec3, n: Vec3| v - n * v.dot(&n);
  let n = normal[seed];
  let y = flat(up, n).try_normalize(1e-4).unwrap_or_else(|| {
    flat(
      if n.x.abs() < 0.9 {
        Vec3::x()
      } else {
        Vec3::z()
      },
      n,
    )
    .normalize()
  });
  let mut x_axis = vec![Vec3::zeros(); groups.len()];
  let mut dist = vec![f32::INFINITY; groups.len()];
  x_axis[seed] = y.cross(&n);
  let to_seed = flat(pos[seed] - origin, n);
  uv[seed] = Vector2::new(to_seed.dot(&x_axis[seed]), to_seed.dot(&y));
  dist[seed] = 0.;

  let group_of = &group_of;
  let neighbors = |p: usize| {
    groups[p].iter().flat_map(move |&v| {
      m.vertices[v].edges.iter().map(move |&e| {
        let [a, b] = m.edges[e].vertices;
        group_of[&if a == v { b } else { a }]
      })
    })
  };
  // Non-negative floats order like their bits.
  let mut heap = BinaryHeap::from([(Reverse(0f32.to_bits()), seed)]);
  let mut settled = vec![false; groups.len()];
  let mut ended = vec![false; groups.len()];
  while let Some((Reverse(d), q)) = heap.pop() {
    if std::mem::replace(&mut settled[q], true) {
      continue;
    }
    // Each settled neighbour proposes a position: its own plus the connecting edge laid flat
    // in its tangent plane (direction from the plane, length kept), and its axes parallel
    // transported here. Averaging the proposals keeps one zigzag route from skewing the chart.
    let (mut pos_sum, mut x_sum, mut w_sum) = (Vector2::zeros(), Vec3::zeros(), 0.);
    for p in neighbors(q).filter(|&p| settled[p] && !ended[p] && p != q) {
      let step = pos[q] - pos[p];
      let len = step.norm();
      let (n_p, x_p) = (normal[p], x_axis[p]);
      let along = flat(step, n_p).try_normalize(1e-12).unwrap_or_default() * len;
      // `rotation_between` is NaN for unit vectors whose dot rounds past 1.
      let turn = Some(n_p.dot(&normal[q]) < 1. - 1e-6)
        .filter(|turned| *turned)
        .and_then(|_| Rotation3::rotation_between(&n_p, &normal[q]))
        .unwrap_or_else(Rotation3::identity);
      let w = 1. / len.max(1e-12);
      pos_sum += (uv[p] + Vector2::new(along.dot(&x_p), along.dot(&n_p.cross(&x_p)))) * w;
      x_sum += turn * x_p * w;
      w_sum += w;
    }
    let d = f32::from_bits(d);
    if w_sum > 0. {
      uv[q] = pos_sum / w_sum;
      x_axis[q] = flat(x_sum, normal[q])
        .try_normalize(1e-6)
        .unwrap_or_else(Vec3::x);
    }
    // Past the far side of a closed shape, routes arriving from opposite directions propose
    // opposite positions and their mean lands back near the origin: the unrolled sheet would
    // have to overlap itself there. The chart ends at such vertices and whatever lies beyond.
    if q != seed && (w_sum == 0. || uv[q].norm() < 0.5 * d) {
      uv[q] = Vector2::repeat(UNREACHED);
      ended[q] = true;
    }
    for p in neighbors(q) {
      let reach = d + (pos[p] - pos[q]).norm();
      if !settled[p] && reach < dist[p] {
        dist[p] = reach;
        heap.push((Reverse(reach.to_bits()), p));
      }
    }
  }
  group_of.iter().map(|(&k, &i)| (k, uv[i])).collect()
}

/// Per loop, the smooth run each segment (from point `i` to `i + 1`) belongs to: the outline is
/// broken into runs wherever it turns by more than ~20°. The distance fields of two runs meet in
/// a crease of the path's distance (a corner's bisector, the ridge between two shapes).
fn path_runs(loops: &[Vec<Vector2<f32>>]) -> Vec<Vec<u32>> {
  const CORNER: f32 = 0.35;
  let mut next = 0;
  loops
    .iter()
    .map(|l| {
      let n = l.len();
      let corner = |i: usize| {
        let (a, b) = (l[i] - l[(i + n - 1) % n], l[(i + 1) % n] - l[i]);
        a.angle(&b) > CORNER
      };
      let mut runs = vec![next; n];
      if let Some(first) = (0..n).find(|&i| corner(i)) {
        for j in 0..n {
          let i = (first + j) % n;
          next += u32::from(corner(i));
          runs[i] = next;
        }
      }
      next += 1;
      runs
    })
    .collect()
}

/// A filled 2D path's segments under a BVH, for signed distance (even-odd, negative inside) and
/// distance to single runs.
pub struct PathIndex {
  /// `(a, b, run)` in loop order.
  segs: Vec<(Vector2<f32>, Vector2<f32>, u32)>,
  /// `(min, max, start, len)`: over `order[start..][..len]` for a leaf (`len > 0`), else over
  /// the child nodes `start` and `start + 1`.
  nodes: Vec<(Vector2<f32>, Vector2<f32>, u32, u32)>,
  order: Vec<u32>,
  by_run: Vec<Vec<u32>>,
}

fn seg_d2(a: Vector2<f32>, b: Vector2<f32>, p: Vector2<f32>) -> f32 {
  let (ab, ap) = (b - a, p - a);
  let t = (ap.dot(&ab) / ab.norm_squared().max(1e-20)).clamp(0., 1.);
  (ap - ab * t).norm_squared()
}

impl PathIndex {
  const LEAF: usize = 4;

  pub fn new(loops: &[Vec<Vector2<f32>>]) -> Self {
    let runs = path_runs(loops);
    let segs: Vec<_> = loops
      .iter()
      .zip(&runs)
      .flat_map(|(l, r)| (0..l.len()).map(move |i| (l[i], l[(i + 1) % l.len()], r[i])))
      .collect();
    let mut by_run: Vec<Vec<u32>> = Vec::new();
    for (i, s) in segs.iter().enumerate() {
      if by_run.len() <= s.2 as usize {
        by_run.resize(s.2 as usize + 1, Vec::new());
      }
      by_run[s.2 as usize].push(i as u32);
    }
    let mut ix = Self {
      order: (0..segs.len() as u32).collect(),
      segs,
      nodes: Vec::new(),
      by_run,
    };
    if !ix.segs.is_empty() {
      ix.nodes.push(Default::default());
      ix.build(0, 0, ix.segs.len());
    }
    ix
  }

  fn build(&mut self, node: usize, start: usize, len: usize) {
    let (mut lo, mut hi) = (
      Vector2::repeat(f32::INFINITY),
      Vector2::repeat(-f32::INFINITY),
    );
    for &i in &self.order[start..start + len] {
      let (a, b, _) = self.segs[i as usize];
      (lo, hi) = (lo.inf(&a).inf(&b), hi.sup(&a).sup(&b));
    }
    if len <= Self::LEAF {
      self.nodes[node] = (lo, hi, start as u32, len as u32);
      return;
    }
    let axis = usize::from(hi.y - lo.y > hi.x - lo.x);
    let segs = &self.segs;
    let mid = |i: &u32| segs[*i as usize].0[axis] + segs[*i as usize].1[axis];
    self.order[start..start + len]
      .select_nth_unstable_by(len / 2, |a, b| mid(a).total_cmp(&mid(b)));
    let child = self.nodes.len();
    self.nodes[node] = (lo, hi, child as u32, 0);
    self.nodes.resize(child + 2, Default::default());
    self.build(child, start, len / 2);
    self.build(child + 1, start + len / 2, len - len / 2);
  }

  /// Smallest squared distance and the first segment (in loop order) at it.
  fn nearest(&self, p: Vector2<f32>) -> (f32, u32) {
    let (mut d2, mut best) = (f32::INFINITY, u32::MAX);
    // A NaN point (a chart that broke down) is at no distance from anything, and can't be culled.
    let (Some(&(lo, hi, ..)), false) = (self.nodes.first(), p.x.is_nan() || p.y.is_nan()) else {
      return (d2, best);
    };
    let box_d2 = |n: u32| {
      let (lo, hi, ..) = self.nodes[n as usize];
      (lo - p)
        .sup(&(p - hi))
        .sup(&Vector2::zeros())
        .norm_squared()
    };
    // From far away (unreached chart vertices) every segment rounds to nearly the same
    // distance and nothing can be culled.
    if box_d2(0) > 256. * (hi - lo).norm_squared() {
      for (i, &(a, b, _)) in self.segs.iter().enumerate() {
        let d = seg_d2(a, b, p);
        if d < d2 {
          (d2, best) = (d, i as u32);
        }
      }
      return (d2, best);
    }
    let mut stack: smallvec::SmallVec<[(u32, f32); 32]> = smallvec::SmallVec::new();
    stack.push((0, 0.));
    while let Some((n, near)) = stack.pop() {
      // The margin keeps every segment whose computed distance could still tie the best.
      if near > d2 * 1.00001 + 1e-30 {
        continue;
      }
      let (_, _, start, len) = self.nodes[n as usize];
      if len == 0 {
        let (l, r) = ((start, box_d2(start)), (start + 1, box_d2(start + 1)));
        let (first, second) = if l.1 <= r.1 { (l, r) } else { (r, l) };
        stack.push(second);
        stack.push(first);
        continue;
      }
      for &i in &self.order[start as usize..(start + len) as usize] {
        let (a, b, _) = self.segs[i as usize];
        let d = seg_d2(a, b, p);
        if d < d2 || (d == d2 && i < best) {
          (d2, best) = (d, i);
        }
      }
    }
    (d2, best)
  }

  pub fn sdf(&self, p: Vector2<f32>) -> f32 {
    if p.x.is_nan() || p.y.is_nan() {
      return f32::INFINITY;
    }
    let mut inside = false;
    let mut stack: smallvec::SmallVec<[u32; 32]> = smallvec::SmallVec::new();
    if !self.nodes.is_empty() {
      stack.push(0);
    }
    while let Some(n) = stack.pop() {
      let (lo, hi, start, len) = self.nodes[n as usize];
      if p.y < lo.y || p.y > hi.y || p.x > hi.x + 1e-4 * hi.x.abs().max(lo.x.abs()).max(1.) {
        continue;
      }
      if len == 0 {
        stack.push(start);
        stack.push(start + 1);
        continue;
      }
      for &i in &self.order[start as usize..(start + len) as usize] {
        let (a, b, _) = self.segs[i as usize];
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b - a).x {
          inside = !inside;
        }
      }
    }
    let d = self.nearest(p).0.sqrt();
    if inside {
      -d
    } else {
      d
    }
  }

  /// Nearest run to `p` (`None` asks for that) or distance to a given one.
  pub fn run_dist(&self, p: Vector2<f32>, run: Option<u32>) -> (u32, f32) {
    match run {
      None => {
        let (d2, i) = self.nearest(p);
        (self.segs.get(i as usize).map_or(0, |s| s.2), d2.sqrt())
      }
      Some(run) => {
        let d2 = self.by_run.get(run as usize).map_or(f32::INFINITY, |segs| {
          segs.iter().fold(f32::INFINITY, |d2, &i| {
            let (a, b, _) = self.segs[i as usize];
            let d = seg_d2(a, b, p);
            if d < d2 {
              d
            } else {
              d2
            }
          })
        });
        (if d2 < f32::INFINITY { run } else { 0 }, d2.sqrt())
      }
    }
  }
}
