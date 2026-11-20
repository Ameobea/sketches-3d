//! Adaptive tessellation of a per-vertex field: faces are bisected (Rivara longest-edge
//! propagation) wherever probing the field at edge midpoints, quarter points, or the centroid
//! disagrees with the corners' linear interpolation by more than `tol`. Splits leave the
//! surface unchanged.

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, EdgeKey, EdgeSplitPos, FaceKey, Vec3, VertexKey},
  LinkedMesh,
};
use nalgebra::Matrix4;
use smallvec::SmallVec;

use crate::ErrorStack;

pub struct Refine {
  pub tol: f32,
  /// World units; edges shorter than this are never split.
  pub min_edge: f32,
  /// World units; edges longer than this are split regardless of the field, so features that fit
  /// between probes can't be missed entirely.
  pub max_edge: f32,
}

pub type Lanes = [f32; 4];

fn lane_dist(a: Lanes, b: Lanes) -> f32 {
  (0..4).map(|i| (a[i] - b[i]).abs()).fold(0., f32::max)
}

fn lerp(a: Lanes, b: Lanes, t: f32) -> Lanes {
  std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// `vals` holds every vertex's value on entry and gains the split vertices'. `sample` receives
/// a local-space position and unit normal; `xf` measures edge lengths in world units. With
/// `straddle`, only faces whose corners or probes fall on both sides of that threshold (lane 0)
/// are refined, so a curved field only gains triangles along its contour.
pub fn refine_field(
  out: &mut LinkedMesh<()>,
  xf: &Matrix4<f32>,
  vals: &mut FxHashMap<VertexKey, Lanes>,
  r: &Refine,
  straddle: Option<f32>,
  mut sample: impl FnMut(Vec3, Vec3) -> Result<Lanes, ErrorStack>,
) -> Result<(), ErrorStack> {
  let pair = |a: VertexKey, b: VertexKey| if a < b { [a, b] } else { [b, a] };
  let world = |p: Vec3| (xf * p.push(1.)).xyz();
  let unit = |n: Vec3| {
    let l = n.norm();
    if l > 1e-12 {
      n / l
    } else {
      Vec3::y()
    }
  };
  let ends =
    |out: &LinkedMesh<()>, ek: EdgeKey| out.edges[ek].vertices.map(|k| out.vertices[k].position);
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
    (world(a) - world(b)).norm()
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
  // Probes at edge midpoints / quarter points, keyed by endpoints so a split edge's quarter
  // samples become its halves' midpoints.
  let mut mid: FxHashMap<[VertexKey; 2], Lanes> = FxHashMap::default();
  let mut quarters: FxHashMap<[VertexKey; 2], [Lanes; 2]> = FxHashMap::default();

  let mut queue: Vec<FaceKey> = out.faces.keys().collect();
  while let Some(fk) = queue.pop() {
    let Some(face) = out.faces.get(fk) else {
      continue;
    };
    let longest = longest_edge(out, &coincident, fk);
    if edge_len(out, longest) < r.min_edge {
      continue;
    }
    let forced = edge_len(out, longest) > r.max_edge;
    let side = |v: Lanes| straddle.is_some_and(|t| v[0] < t);
    let s0 = side(vals[&face.vertices[0]]);
    let mut cross = face.vertices.iter().any(|k| side(vals[k]) != s0);
    let mut nonlinear = false;
    for &ek in &face.edges {
      let [v0, v1] = out.edges[ek].vertices;
      let [a, b] = ends(out, ek);
      if edge_len(out, ek) < r.min_edge {
        continue;
      }
      let (f0, f1) = (vals[&v0], vals[&v1]);
      let n = unit(edge_normal(out, ek));
      let m = match mid.get(&pair(v0, v1)) {
        Some(m) => *m,
        None => {
          let m = sample((a + b) * 0.5, n)?;
          mid.insert(pair(v0, v1), m);
          m
        }
      };
      cross |= side(m) != s0;
      nonlinear |= lane_dist(m, lerp(f0, f1, 0.5)) > r.tol;
      // Steep edges get quarter probes too: an S-shaped transition centred on an edge puts its
      // midpoint right on the chord.
      if lane_dist(f0, f1) > r.tol {
        let [q1, q3] = match quarters.get(&pair(v0, v1)) {
          Some(q) => *q,
          None => {
            let q = [sample(a.lerp(&b, 0.25), n)?, sample(a.lerp(&b, 0.75), n)?];
            quarters.insert(pair(v0, v1), q);
            q
          }
        };
        cross |= side(q1) != s0 || side(q3) != s0;
        nonlinear |=
          lane_dist(q1, lerp(f0, f1, 0.25)) > r.tol || lane_dist(q3, lerp(f0, f1, 0.75)) > r.tol;
      }
    }
    // Corners on creases carry compromise normals, so a big face can be wrong inside while every
    // edge midpoint agrees with its endpoints; the centroid catches that.
    if !nonlinear {
      let [p, q, r_] = face.vertices.map(|k| out.vertices[k].position);
      let mean = face.vertices.iter().fold([0.; 4], |acc, k| {
        std::array::from_fn(|i| acc[i] + vals[k][i] / 3.)
      });
      let c = sample((p + q + r_) / 3., unit((q - p).cross(&(r_ - p))))?;
      cross |= side(c) != s0;
      nonlinear = lane_dist(c, mean) > r.tol;
    }
    if !(forced || (nonlinear && (straddle.is_none() || cross))) {
      continue;
    }
    // Rivara bisection: an edge is only split once it's the longest edge of every face on it
    // (twins included), walking to neighbours' longer edges first, which keeps the smallest
    // angle bounded.
    let mut stack = vec![longest];
    while let Some(&x) = stack.last() {
      let g = group(out, &coincident, x);
      let blocker = g
        .iter()
        .flat_map(|&e| out.edges[e].faces.iter().copied())
        .map(|f| longest_edge(out, &coincident, f))
        .find(|y| !g.contains(y));
      if let Some(y) = blocker {
        stack.push(y);
        continue;
      }
      let mut shared_mid = None;
      for &e in &g {
        let [v0, v1] = out.edges[e].vertices;
        let [a, b] = ends(out, e);
        let n = unit(edge_normal(out, e));
        let m = match mid.get(&pair(v0, v1)) {
          Some(m) => *m,
          None => sample((a + b) * 0.5, n)?,
        };
        let vm = out.split_edge_cb(
          e,
          EdgeSplitPos::middle(),
          DisplacementNormalMethod::Interpolate,
          |_, _, _, new_faces| queue.extend(new_faces),
        );
        let p = *shared_mid.get_or_insert(out.vertices[vm].position);
        out.vertices[vm].position = p;
        coincident.entry(pos_key(p)).or_default().push(vm);
        vals.insert(vm, m);
        if let Some([q1, q3]) = quarters.remove(&pair(v0, v1)) {
          mid.insert(pair(v0, vm), q1);
          mid.insert(pair(vm, v1), q3);
        }
      }
      stack.pop();
    }
  }
  Ok(())
}
