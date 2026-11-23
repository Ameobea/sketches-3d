//! Adaptive tessellation of a per-vertex field: faces are bisected (Rivara longest-edge
//! propagation) wherever probing the field at edge midpoints, quarter points, or the centroid
//! disagrees with the corners' linear interpolation by more than `tol`. Splits leave the
//! surface unchanged.

use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{DisplacementNormalMethod, EdgeKey, EdgeSplitPos, FaceKey, Vec3, VertexKey},
  LinkedMesh,
};
use nalgebra::Matrix4;
use smallvec::SmallVec;

use crate::ErrorStack;

#[derive(Clone, Copy)]
pub struct Refine {
  pub tol: f32,
  /// World units; edges shorter than this are never split.
  pub min_edge: f32,
  /// World units; edges longer than this are split regardless of the field, so features that fit
  /// between probes can't be missed entirely.
  pub max_edge: f32,
}

pub type Lanes = [f32; 4];

/// Where a probe lands: on an edge (listed first, followed by its coincident seam twins) or
/// inside a face.
pub enum Site<'a> {
  Edge(&'a [EdgeKey]),
  Face(FaceKey),
}

impl Site<'_> {
  /// Area-weighted over the edge's own faces: the normal this point would get as a `tessellate`
  /// vertex, rather than the endpoints' blend, which tilts into the surface beside creases.
  pub fn normal(&self, m: &LinkedMesh<()>) -> Vec3 {
    let area_normal = |f: FaceKey| {
      let [p, q, r] = m.faces[f].vertices.map(|k| m.vertices[k].position);
      (q - p).cross(&(r - p))
    };
    let n: Vec3 = match self {
      Site::Edge(g) => m.edges[g[0]].faces.iter().map(|&f| area_normal(f)).sum(),
      Site::Face(f) => area_normal(*f),
    };
    let l = n.norm();
    if l > 1e-12 {
      n / l
    } else {
      Vec3::y()
    }
  }
}

fn lane_dist(a: Lanes, b: Lanes) -> f32 {
  (0..4).map(|i| (a[i] - b[i]).abs()).fold(0., f32::max)
}

fn lerp(a: Lanes, b: Lanes, t: f32) -> Lanes {
  std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// `vals` holds every vertex's value on entry and gains the split vertices'. `sample` receives
/// the mesh, a local-space position on it, and the probe's site; `xf` measures edge lengths in
/// world units. With
/// `straddle`, only faces whose corners or probes fall on both sides of that threshold (lane 0)
/// are refined, so a curved field only gains triangles along its contour.
pub fn refine_field(
  out: &mut LinkedMesh<()>,
  xf: &Matrix4<f32>,
  vals: &mut FxHashMap<VertexKey, Lanes>,
  r: &Refine,
  straddle: Option<f32>,
  sample: impl FnMut(&LinkedMesh<()>, Vec3, Site) -> Result<Lanes, ErrorStack>,
) -> Result<(), ErrorStack> {
  refine_region(out, xf, vals, r, straddle, None, None, sample)
}

/// What a crease hook did with an edge.
pub enum Crease {
  None,
  /// Split it at the crease, creating this vertex between these ends.
  Split(VertexKey, [VertexKey; 2]),
  /// Slid this end of it onto the crease.
  Slid(VertexKey),
}

/// What a crease hook is asked about: a face's edge before the face is bisected, or a
/// bisection's new vertex (with the ends it lies between and the split faces' other corners),
/// which it may slide.
pub enum Offer {
  Edge(EdgeKey),
  Bisected(VertexKey, [VertexKey; 2], [VertexKey; 2]),
}

pub type Region<'a, 'b> = &'a mut Option<&'b mut FxHashSet<FaceKey>>;
pub type CreaseHook<'a> = &'a mut dyn FnMut(
  &mut LinkedMesh<()>,
  &mut FxHashMap<VertexKey, Lanes>,
  Region,
  Offer,
) -> Result<Crease, ErrorStack>;

/// [`refine_field`] over just the faces in `region` (kept current as faces split; only its
/// vertices need entries in `vals`), returning each bisection's vertex and the ends it lies
/// between. A face found off by more than the tolerance is first offered edge by edge to `crease`:
/// where the field has a crease through it, a vertex there (the hook's to place, and to give a
/// value) fixes what no number of midpoints would.
pub fn refine_region(
  out: &mut LinkedMesh<()>,
  xf: &Matrix4<f32>,
  vals: &mut FxHashMap<VertexKey, Lanes>,
  r: &Refine,
  straddle: Option<f32>,
  mut region: Option<&mut FxHashSet<FaceKey>>,
  mut crease: Option<CreaseHook>,
  mut sample: impl FnMut(&LinkedMesh<()>, Vec3, Site) -> Result<Lanes, ErrorStack>,
) -> Result<(), ErrorStack> {
  let pair = |a: VertexKey, b: VertexKey| if a < b { [a, b] } else { [b, a] };
  let world = |p: Vec3| (xf * p.push(1.)).xyz();
  let ends =
    |out: &LinkedMesh<()>, ek: EdgeKey| out.edges[ek].vertices.map(|k| out.vertices[k].position);
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
    let mut g: SmallVec<[EdgeKey; 2]> = c0
      .iter()
      .flat_map(|&a| c1.iter().map(move |&b| [a, b]))
      .filter_map(|ab| out.get_edge_key(ab))
      .filter(|&t| t != ek && out.edges[t].faces.len() == 1)
      .collect();
    g.insert(0, ek);
    g
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
    if region.as_ref().is_some_and(|r| !r.contains(&fk)) {
      continue;
    }
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
      let g = group(out, &coincident, ek);
      let m = match mid.get(&pair(v0, v1)) {
        Some(m) => *m,
        None => {
          let m = sample(out, (a + b) * 0.5, Site::Edge(&g))?;
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
            // Lerped from the same end on every seam twin, so their probes are bit-identical.
            let flip = pos_key(a) > pos_key(b);
            let (lo, hi) = if flip { (b, a) } else { (a, b) };
            let mut q = [
              sample(out, lo.lerp(&hi, 0.25), Site::Edge(&g))?,
              sample(out, lo.lerp(&hi, 0.75), Site::Edge(&g))?,
            ];
            if flip {
              q.reverse();
            }
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
      let c = sample(out, (p + q + r_) / 3., Site::Face(fk))?;
      cross |= side(c) != s0;
      nonlinear = lane_dist(c, mean) > r.tol;
    }
    if !(forced || (nonlinear && (straddle.is_none() || cross))) {
      continue;
    }
    if let (Some(crease), false) = (&mut crease, forced) {
      let around = |out: &LinkedMesh<()>, v: VertexKey| -> SmallVec<[FaceKey; 8]> {
        let edges = out.vertices[v].edges.iter();
        edges
          .flat_map(|&e| out.edges[e].faces.iter().copied())
          .collect()
      };
      let mut handled = false;
      for ek in face.edges {
        match crease(out, vals, &mut region, Offer::Edge(ek))? {
          Crease::None => continue,
          Crease::Split(vm, _) => {
            let p = out.vertices[vm].position;
            coincident.entry(pos_key(p)).or_default().push(vm);
            queue.extend(around(out, vm));
          }
          Crease::Slid(k) => {
            for &e in &out.vertices[k].edges {
              let [v0, v1] = out.edges[e].vertices;
              mid.remove(&pair(v0, v1));
              quarters.remove(&pair(v0, v1));
            }
            queue.extend(around(out, k));
          }
        }
        handled = true;
        break;
      }
      if handled {
        continue;
      }
    }
    // Rivara bisection: an edge is only split once it's the longest edge of every face on it
    // (twins included), walking to neighbours' longer edges first, which keeps the smallest
    // angle bounded.
    let mut stack = vec![longest];
    while let Some(&x) = stack.last() {
      // Splitting a face re-keys its boundary edges; the faces that replaced it are queued.
      if !out.edges.contains_key(x) {
        stack.pop();
        continue;
      }
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
        let m = match mid.get(&pair(v0, v1)) {
          Some(m) => *m,
          None => sample(out, (a + b) * 0.5, Site::Edge(&group(out, &coincident, e)))?,
        };
        let apexes: SmallVec<[VertexKey; 2]> = (out.edges[e].faces.iter())
          .filter_map(|&f| {
            out.faces[f]
              .vertices
              .into_iter()
              .find(|k| *k != v0 && *k != v1)
          })
          .collect();
        let vm = out.split_edge_cb(
          e,
          EdgeSplitPos::middle(),
          DisplacementNormalMethod::Interpolate,
          |_, old, _, new_faces| {
            if let Some(region) = &mut region {
              if region.remove(&old) {
                region.extend(new_faces);
              }
            }
            queue.extend(new_faces)
          },
        );
        let p = *shared_mid.get_or_insert(out.vertices[vm].position);
        out.vertices[vm].position = p;
        coincident.entry(pos_key(p)).or_default().push(vm);
        vals.insert(vm, m);
        let halves = quarters.remove(&pair(v0, v1));
        if let (Some(crease), &[c, d]) = (&mut crease, &apexes[..]) {
          let offer = Offer::Bisected(vm, [v0, v1], [c, d]);
          if let Crease::Slid(_) = crease(out, vals, &mut region, offer)? {
            continue;
          }
        }
        if let Some([q1, q3]) = halves {
          mid.insert(pair(v0, vm), q1);
          mid.insert(pair(vm, v1), q3);
        }
      }
      stack.pop();
    }
  }
  Ok(())
}
