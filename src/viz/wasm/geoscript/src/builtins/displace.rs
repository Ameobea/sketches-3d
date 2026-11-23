use std::{
  cell::{Cell, RefCell},
  rc::Rc,
};

use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  bvh::TriBvh,
  linked_mesh::{
    mesh_flags, Arity, Channel, DisplacementNormalMethod, EdgeKey, EdgeSplitPos, FaceKey, Vec3,
    VertexKey,
  },
  LinkedMesh,
};
use nalgebra::Matrix4;
use smallvec::SmallVec;

use crate::{
  builtins::vertex_attrs::{attr_name, cut_by_field, refine_kwargs, FieldSrc},
  guards::extract_guards,
  mesh_ops::{
    displace::{coincident_groups, edge_dir, extrude_region, face_dir, group_dir},
    mesh_ops::refresh_stale_normals,
    refine::{refine_field, refine_region, Crease, Lanes, Offer, Refine, Region, Site},
    surface_chart::surface_chart,
    tessellate_polygon::clear_straddle,
  },
  vertex_cb::VertexCb,
  ArgRef, Callable, ErrorStack, EvalCtx, ManifoldHandle, MeshHandle, Sym, Value,
};

fn handle(src: &MeshHandle, mesh: LinkedMesh<()>) -> Value {
  Value::Mesh(Rc::new(MeshHandle {
    mesh: Rc::new(mesh),
    transform: src.transform,
    manifold_handle: Rc::new(ManifoldHandle::new_empty()),
    aabb: Default::default(),
    bvh: Default::default(),
    material: src.material.clone(),
  }))
}

fn wrap(ctx: &EvalCtx, src: &MeshHandle, mut mesh: LinkedMesh<()>) -> Value {
  refresh_stale_normals(&src.mesh, &mut mesh, ctx);
  handle(src, mesh)
}

/// A height callback's args: `|pos, normal|`, plus for `emboss` the selecting field's value at
/// the point (`dist`: for a signed distance field, minus the distance in from the region's edge).
enum Height<'a> {
  Const(f32),
  Cb(VertexCb<'a>),
}

fn height_args(p: Vec3, n: Vec3, dist: Option<f32>) -> SmallVec<[Value; 3]> {
  [Value::Vec3(p), Value::Vec3(n)]
    .into_iter()
    .chain(dist.map(Value::Float))
    .collect()
}

impl<'a> Height<'a> {
  fn new(
    ctx: &EvalCtx,
    builtin: &'static str,
    v: &'a Value,
    mesh: &LinkedMesh<()>,
  ) -> Result<Self, ErrorStack> {
    let fixed: &[&str] = if builtin == "emboss" {
      &["pos", "normal", "dist"]
    } else {
      &["pos", "normal"]
    };
    Ok(match v {
      Value::Callable(cb) => Self::Cb(VertexCb::new(ctx, builtin, cb, fixed, mesh)?),
      v => Self::Const(v.as_float().unwrap()),
    })
  }

  fn num(v: Value) -> Result<f32, ErrorStack> {
    v.as_float().filter(|h| h.is_finite()).ok_or_else(|| {
      ErrorStack::new(format!(
        "height callback must return a finite number; got {v:?}"
      ))
    })
  }

  fn at_vertex(
    &self,
    ctx: &EvalCtx,
    m: &LinkedMesh<()>,
    k: VertexKey,
    n: Vec3,
    dist: Option<f32>,
  ) -> Result<f32, ErrorStack> {
    match self {
      Self::Const(h) => Ok(*h),
      Self::Cb(cb) => {
        Self::num(cb.invoke(ctx, &height_args(m.vertices[k].position, n, dist), m, k)?)
      }
    }
  }

  fn reads_normal(&self) -> bool {
    matches!(self, Self::Cb(cb) if cb.reads_fixed(1))
  }

  fn at(&self, ctx: &EvalCtx, p: Vec3, n: Vec3, dist: Option<f32>) -> Result<f32, ErrorStack> {
    match self {
      Self::Const(h) => Ok(*h),
      Self::Cb(cb) => Self::num(cb.invoke_fixed(ctx, &height_args(p, n, dist))?),
    }
  }
}

fn unit(d: Vec3) -> Vec3 {
  d.try_normalize(1e-6).unwrap_or_default()
}

fn refine_needs_pos_only(builtin: &str) -> ErrorStack {
  ErrorStack::new(format!(
    "`{builtin}` with `refine` probes the field between vertices, so the callback may only read \
     `pos` and `normal` (no `ix` or attribute bag keys)"
  ))
}

/// A height's creases, by (undisplaced) position.
#[derive(Default)]
struct Creases {
  /// Vertices on a crease, with a bit per guard that vanishes there.
  verts: FxHashMap<[u32; 3], u16>,
  /// Edges running along one, as their ends in ascending order.
  edges: FxHashSet<[[u32; 3]; 2]>,
}

impl Creases {
  fn along(&self, m: &LinkedMesh<()>, ends: [VertexKey; 2]) -> bool {
    let [a, b]: [[u32; 3]; 2] = ends.map(|k| m.vertices[k].position.map(f32::to_bits).into());
    self.edges.contains(&[a.min(b), a.max(b)])
  }
}

type Guards = SmallVec<[f32; 8]>;

/// Guard slot of the distance field's own creases; the callback's guards take the ones below.
const FEATURE: usize = 15;

/// Where an `emboss` height callback's `dist` comes from.
enum Dist<'a> {
  Field(&'a FieldSrc<'a>, f32),
  #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
  Mask(Rc<TriBvh>),
}

impl Dist<'_> {
  fn at_vertex(&self, ctx: &EvalCtx, m: &LinkedMesh<()>, k: VertexKey) -> Result<f32, ErrorStack> {
    match self {
      Self::Field(src, thr) => Ok(src.at_vertex(ctx, m, k)? - thr),
      Self::Mask(_) => self.at(ctx, m, m.vertices[k].position, &Site::Edge(&[])),
    }
  }

  fn at(&self, ctx: &EvalCtx, m: &LinkedMesh<()>, p: Vec3, site: &Site) -> Result<f32, ErrorStack> {
    match self {
      Self::Field(src, thr) => Ok(src.at(ctx, m, p, site)? - thr),
      Self::Mask(bvh) => Ok(bvh.signed_distance(&p, f32::INFINITY, true).unwrap_or(0.)),
    }
  }
}

impl Dist<'_> {
  fn has_features(&self) -> bool {
    matches!(self, Self::Field(src, _) if src.has_features())
  }

  fn feature(&self, m: &LinkedMesh<()>, k: VertexKey) -> Option<u32> {
    match self {
      Self::Field(src, _) => src.feature(m, k),
      Self::Mask(_) => None,
    }
  }

  fn feature_gap(&self, m: &LinkedMesh<()>, p: Vec3, site: &Site, a: u32, b: u32) -> Option<f32> {
    match self {
      Self::Field(src, _) => src.feature_gap(m, p, site, a, b),
      Self::Mask(_) => None,
    }
  }
}

fn dist_at(
  dist: Option<&Dist>,
  ctx: &EvalCtx,
  m: &LinkedMesh<()>,
  p: Vec3,
  site: &Site,
) -> Result<Option<f32>, ErrorStack> {
  dist.map(|d| d.at(ctx, m, p, site)).transpose()
}

/// Tessellates for a height callback. Rivara refinement brings the height's deviation from
/// linear under `r`'s tolerance; then, as in `embed_path`'s crease post-pass, every edge that
/// crosses a zero of one of the callback's guards (`guards.rs`: where its slope or value
/// breaks) is split exactly there, which chains the crease into mesh edges. `vals` holds
/// the height (lane 0) of the region's vertices and gains the new ones; returns the positions of
/// the vertices on creases.
#[allow(clippy::too_many_arguments)]
fn refine_height(
  ctx: &EvalCtx,
  out: &mut LinkedMesh<()>,
  xf: &Matrix4<f32>,
  height: &Height,
  callback: &Value,
  vals: &mut FxHashMap<VertexKey, Lanes>,
  r: &Refine,
  mut region: Option<&mut FxHashSet<FaceKey>>,
  dist: Option<&Dist>,
) -> Result<Creases, ErrorStack> {
  let wrap_err = |err: ErrorStack| err.wrap("error calling height callback");
  // The mitered normals cost more than most callbacks.
  let reads_normal = height.reads_normal();
  let normal_at = |out: &LinkedMesh<()>, k: VertexKey| match reads_normal {
    true => unit(group_dir(out, &[k], |_| true)),
    false => Vec3::zeros(),
  };
  let sample = |m: &LinkedMesh<()>, p: Vec3, site: Site| -> Result<Lanes, ErrorStack> {
    let d = dist_at(dist, ctx, m, p, &site)?;
    let n = match site {
      _ if !reads_normal => Vec3::zeros(),
      Site::Edge(g) => unit(edge_dir(m, g)),
      Site::Face(f) => unit(face_dir(m, f)),
    };
    Ok([height.at(ctx, p, n, d).map_err(wrap_err)?, 0., 0., 0.])
  };

  let creases = RefCell::new(Creases::default());
  let closure = match callback {
    Value::Callable(cb) => match &**cb {
      Callable::Closure(closure) => Some(closure),
      _ => None,
    },
    _ => None,
  };
  let Some(closure) = closure else {
    refine_region(out, xf, vals, r, None, region, None, sample)?;
    return Ok(creases.into_inner());
  };
  let guards = extract_guards(ctx, closure).map(|g| Rc::new(Callable::Closure(g)));
  if guards.is_none() && !dist.is_some_and(Dist::has_features) {
    refine_region(out, xf, vals, r, None, region, None, sample)?;
    return Ok(creases.into_inner());
  }
  let fixed: &[&str] = match dist {
    Some(_) => &["pos", "normal", "dist"],
    None => &["pos", "normal"],
  };
  let guards = guards
    .as_ref()
    .map(|g| VertexCb::new(ctx, "displace", g, fixed, out))
    .transpose()?;
  // The callback's guards at a point (a vector-valued one counts per component), at most
  // `FEATURE` of them. `None` (the closure erroring off the surface it was written for) just
  // skips that edge.
  let eval = |p: Vec3, n: Vec3, d: Option<f32>| -> Option<Guards> {
    let Some(guards) = &guards else {
      return Some(Guards::new());
    };
    let out = guards.invoke_fixed(ctx, &height_args(p, n, d)).ok()?;
    let mut all = Guards::new();
    for v in out.as_sequence()?.consume(ctx) {
      match v.ok()? {
        Value::Vec2(v) => all.extend_from_slice(v.as_slice()),
        Value::Vec3(v) => all.extend_from_slice(v.as_slice()),
        v => all.push(v.as_float().unwrap_or(f32::NAN)),
      }
    }
    all.truncate(FEATURE);
    Some(all)
  };
  let bits = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
  let at_vertex: RefCell<FxHashMap<VertexKey, Option<Guards>>> = RefCell::default();
  // Vertices by position, current while creases are being placed.
  let twins: RefCell<FxHashMap<[u32; 3], SmallVec<[VertexKey; 2]>>> = RefCell::default();
  let guards_at = |out: &LinkedMesh<()>, k: VertexKey| {
    if let Some(g) = at_vertex.borrow().get(&k) {
      return g.clone();
    }
    let n = normal_at(out, k);
    let g = dist
      .map(|d| d.at_vertex(ctx, out, k))
      .transpose()
      .ok()
      .and_then(|d| eval(out.vertices[k].position, n, d));
    at_vertex.borrow_mut().insert(k, g.clone());
    g
  };
  let feature_at: RefCell<FxHashMap<VertexKey, Option<u32>>> = RefCell::default();
  let feature = |out: &LinkedMesh<()>, k: VertexKey| {
    let dist = dist?;
    *feature_at
      .borrow_mut()
      .entry(k)
      .or_insert_with(|| dist.feature(out, k))
  };
  let in_region = |region: &Option<&mut FxHashSet<FaceKey>>, faces: &[FaceKey]| {
    region
      .as_ref()
      .is_none_or(|r| faces.iter().any(|f| r.contains(f)))
  };

  let proven = Cell::new(0u16);
  // One edge: where a crease of the height crosses it, split it there (or slide its near end
  // onto the crease). `sweep` 0 judges by the edge's ends alone; 1 is for the guards that turned
  // out to mark real creases, on edges whose ends agree, which are then sampled inside too; 2
  // is 0 restricted to those guards.
  let try_edge = |out: &mut LinkedMesh<()>,
                  vals: &mut FxHashMap<VertexKey, Lanes>,
                  region: Region,
                  ek: EdgeKey,
                  sweep: u8,
                  slide: Option<VertexKey>|
   -> Result<Crease, ErrorStack> {
    let Some(e) = out.edges.get(ek) else {
      return Ok(Crease::None);
    };
    if !in_region(region, &e.faces) {
      return Ok(Crease::None);
    }
    // Low to high, so the coincident copies of a seam edge split at bit-identical points.
    let [mut a, mut b] = e.vertices;
    if bits(out.vertices[a].position) > bits(out.vertices[b].position) {
      std::mem::swap(&mut a, &mut b);
    }
    let (pa, pb) = (out.vertices[a].position, out.vertices[b].position);
    let (Some(ga), Some(gb)) = (guards_at(out, a), guards_at(out, b)) else {
      return Ok(Crease::None);
    };
    let n = match reads_normal {
      true => unit(edge_dir(out, &[ek])),
      false => Vec3::zeros(),
    };
    let site = [ek];
    let site = Site::Edge(&site);
    let d_at = |t: f32| dist_at(dist, ctx, out, pa.lerp(&pb, t), &site);
    // Guard `FEATURE` is the distance field's own: zero where the nearest feature changes
    // between this edge's ends.
    let features = feature(out, a)
      .zip(feature(out, b))
      .filter(|(fa, fb)| fa != fb);
    let guard = |i: usize, t: f32| -> Result<Option<f32>, ErrorStack> {
      let p = pa.lerp(&pb, t);
      Ok(match (i, features) {
        (FEATURE, Some((fa, fb))) => dist.and_then(|d| d.feature_gap(out, p, &site, fa, fb)),
        (FEATURE, None) => None,
        _ => eval(p, n, d_at(t)?).and_then(|g| g.get(i).copied()),
      })
    };
    let len = (pb - pa).norm();
    // How steeply a guard changes here: its gradient fitted over a face on this edge.
    let apex = out.faces[e.faces[0]]
      .vertices
      .into_iter()
      .find(|k| *k != a && *k != b);
    let steepness = |i: usize| -> Option<f32> {
      let c = apex?;
      let pc = out.vertices[c].position;
      let (ends, g_c) = if i == FEATURE {
        let (fa, fb) = features?;
        let to_c = [out.get_edge_key([a, c])?];
        let gap = |p: Vec3, site: &Site| dist?.feature_gap(out, p, site, fa, fb);
        (
          (gap(pa, &site)?, gap(pb, &site)?),
          gap(pc, &Site::Edge(&to_c))?,
        )
      } else {
        ((ga[i], gb[i]), *guards_at(out, c)?.get(i)?)
      };
      let (e1, e2) = (pb - pa, pc - pa);
      let (a11, a12, a22) = (e1.dot(&e1), e1.dot(&e2), e2.dot(&e2));
      let det = a11 * a22 - a12 * a12;
      let (r1, r2) = (ends.1 - ends.0, g_c - ends.0);
      let grad = (e1 * (r1 * a22 - r2 * a12) + e2 * (r2 * a11 - r1 * a12)) / det;
      Some(grad.norm()).filter(|s| s.is_finite() && *s > 0.)
    };
    // The callback's guards at the edge's quarter points, evaluated once for all of them.
    let inside: RefCell<Option<[Option<Guards>; 3]>> = RefCell::default();
    let inside_at = |i: usize, q: usize| -> Result<Option<f32>, ErrorStack> {
      if inside.borrow().is_none() {
        let mut all = [None, None, None];
        for (slot, t) in all.iter_mut().zip([0.25, 0.5, 0.75]) {
          *slot = eval(pa.lerp(&pb, t), n, d_at(t)?);
        }
        *inside.borrow_mut() = Some(all);
      }
      Ok(
        inside.borrow().as_ref().unwrap()[q]
          .as_ref()
          .and_then(|g| g.get(i).copied()),
      )
    };
    // The guard at the edge's ends, and (second sweep) at its quarter points when both ends
    // sit closer to its zero than it changes over half the edge: it may then cross twice in
    // between (a strip between two branches of a crease, narrower than the triangles), which
    // the ends alone can't show. `embed_path` samples every edge this way.
    let samples = |i: usize| -> Result<SmallVec<[(f32, f32); 5]>, ErrorStack> {
      let (g0, g1) = if i == FEATURE {
        match (guard(i, 0.)?, guard(i, 1.)?) {
          (Some(g0), Some(g1)) => (g0, g1),
          _ => return Ok(SmallVec::new()),
        }
      } else {
        (ga[i], gb[i])
      };
      let mut at = SmallVec::from_slice(&[(0., g0)]);
      let near = || steepness(i).is_some_and(|s| g0.abs().min(g1.abs()) < 0.5 * s * len);
      if sweep == 1 && near() {
        for (q, t) in [0.25, 0.5, 0.75].into_iter().enumerate() {
          if let Some(g) = inside_at(i, q)? {
            at.push((t, g));
          }
        }
      }
      at.push((1., g1));
      Ok(at)
    };
    // A guard's zero is only where the height *may* break (the unused branch of a `min`, a
    // hash's `floor`, two runs of one smooth outline), so a crossing counts only if the
    // height's slope really jumps there: its second difference over a tenth of the edge, about
    // the guard's secant root.
    const SPAN: f32 = 0.05;
    const MIN_SLOPE_JUMP: f32 = 0.15;
    let bends = |t: f32| -> Result<f32, ErrorStack> {
      let span = SPAN.min(t).min(1. - t);
      let h = |t: f32| {
        height
          .at(ctx, pa.lerp(&pb, t), n, d_at(t)?)
          .map_err(wrap_err)
      };
      Ok(((h(t + span)? + h(t - span)? - 2. * h(t)?) / (span * len)).abs())
    };
    // The jump seen along the edge is the crease's own times the sine of the angle they cross
    // at, so a crossing nearly along the crease would pass for no crease and leave a gap in its
    // chain. That sine is how much of the guard's gradient lies along the edge.
    let squareness = |i: usize, (t0, g0): (f32, f32), (t1, g1): (f32, f32)| {
      let along = ((g1 - g0) / ((t1 - t0) * len)).abs();
      steepness(i).map_or(1., |s| along / s).clamp(0.2, 1.)
    };
    let root_of = |i: usize, (mut lo, mut g_lo): (f32, f32), mut hi: f32| {
      for _ in 0..16 {
        let mid = (lo + hi) * 0.5;
        let Some(g) = guard(i, mid)? else {
          break;
        };
        if g_lo * g <= 0. {
          hi = mid;
        } else {
          (lo, g_lo) = (mid, g);
        }
      }
      Ok::<_, ErrorStack>((lo + hi) * 0.5)
    };
    let mut crossing = None;
    'guards: for i in (0..ga.len().min(gb.len())).chain(features.map(|_| FEATURE)) {
      let known = i == FEATURE || proven.get() & (1 << i) != 0;
      if (sweep == 1 && i == FEATURE) || (sweep != 0 && !known) {
        continue;
      }
      let at = samples(i)?;
      if sweep == 1 && at.len() == 2 {
        continue;
      }
      for w in at.windows(2) {
        let ((t0, g0), (t1, g1)) = (w[0], w[1]);
        if !clear_straddle(g0, g1) {
          continue;
        }
        let enough = MIN_SLOPE_JUMP * squareness(i, w[0], w[1]);
        // Judged at the guard's secant root; where the guard itself bends along the edge that
        // can miss the crease, so one already known to be real is tracked down first (over an
        // `emboss` region; too many callback calls for a whole displaced mesh).
        let mut root = None;
        let mut found = bends(t0 + g0 / (g0 - g1) * (t1 - t0))? > enough;
        if !found && region.is_some() && proven.get() & (1 << i) != 0 {
          root = Some(root_of(i, w[0], t1)?);
          found = bends(root.unwrap())? > enough;
        }
        if found {
          proven.set(proven.get() | 1 << i);
          crossing = Some((
            i,
            match root {
              Some(t) => t,
              None => root_of(i, w[0], t1)?,
            },
          ));
          break 'guards;
        }
      }
    }
    let Some((i, t)) = crossing else {
      return Ok(Crease::None);
    };
    // A crossing this close to a vertex of the edge's faces would leave a sliver (whose own
    // edges cross again), so that vertex slides onto the crease instead. It stays put only where
    // sliding would disturb something else: a border or seam, the region's outline, another
    // crease (such an end still counts as on this one, a little up one side)...
    let target = pa.lerp(&pb, t);
    let near = |k: VertexKey| {
      (out.vertices[k].position - target).norm() < (0.1 * r.min_edge).max(0.02 * len)
    };
    let corners = e.faces.iter().filter_map(|&f| {
      out.faces[f]
        .vertices
        .into_iter()
        .find(|k| *k != a && *k != b)
    });
    if let Some(k) = slide.or_else(|| [a, b].into_iter().chain(corners).find(|&k| near(k))) {
      // ...or where it would flip or flatten one of the vertex's faces.
      let keeps_shape = |f: FaceKey| {
        let [p0, p1, p2] = out.faces[f].vertices.map(|v| out.vertices[v].position);
        let [q0, q1, q2] = out.faces[f].vertices.map(|v| {
          if v == k {
            target
          } else {
            out.vertices[v].position
          }
        });
        let (before, after) = ((p1 - p0).cross(&(p2 - p0)), (q1 - q0).cross(&(q2 - q0)));
        // ...or carry it out of a face's plane, off the surface.
        let lift = before.dot(&(target - out.vertices[k].position)).abs();
        before.dot(&after) > 0.25 * before.norm_squared()
          && lift < 0.1 * before.norm() * (target - out.vertices[k].position).norm()
      };
      let around = || {
        let edges = out.vertices[k].edges.iter();
        edges.map(|&e| &out.edges[e].faces)
      };
      let pinned = creases
        .borrow()
        .verts
        .contains_key(&bits(out.vertices[k].position))
        || around().any(|faces| {
          faces.len() != 2
            || region
              .as_ref()
              .is_some_and(|r| !faces.iter().all(|f| r.contains(f)))
        });
      let free = !pinned && around().flatten().all(|&f| keeps_shape(f));
      if free {
        out.vertices[k].position = target;
        for ch in out.vertex_channels.values_mut() {
          if let (Some(x), Some(y)) = (ch.get(a), ch.get(b)) {
            ch.set(k, std::array::from_fn(|l| x[l] + (y[l] - x[l]) * t));
          }
        }
        at_vertex.borrow_mut().remove(&k);
        feature_at.borrow_mut().remove(&k);
        let n = normal_at(out, k);
        let d = dist.map(|d| d.at_vertex(ctx, out, k)).transpose()?;
        let h = height.at_vertex(ctx, out, k, n, d).map_err(wrap_err)?;
        vals.insert(k, [h, 0., 0., 0.]);
      }
      if free || (pinned && (k == a || k == b)) {
        *creases
          .borrow_mut()
          .verts
          .entry(bits(out.vertices[k].position))
          .or_default() |= 1 << i;
      }
      return Ok(if free { Crease::Slid(k) } else { Crease::None });
    }
    let d = d_at(t)?;
    // A seam edge's coincident copies split with it, at the one point.
    let mut copies: SmallVec<[(EdgeKey, VertexKey); 2]> = SmallVec::from_slice(&[(ek, a)]);
    if e.faces.len() == 1 {
      let twins = twins.borrow();
      let (ta, tb) = (&twins[&bits(pa)], &twins[&bits(pb)]);
      let pairs = ta.iter().flat_map(|&x| tb.iter().map(move |&y| (x, y)));
      copies.extend(pairs.filter_map(|(x, y)| {
        let k = out.get_edge_key([x, y])?;
        (k != ek && out.edges[k].faces.len() == 1).then_some((k, x))
      }));
    }
    let h = height.at(ctx, target, n, d).map_err(wrap_err)?;
    let mut vm = None;
    for (k, start) in copies {
      let v = out.split_edge_cb(
        k,
        EdgeSplitPos {
          pos: t,
          start_vtx_key: start,
        },
        DisplacementNormalMethod::Interpolate,
        |_, old, _, new_faces| {
          if let Some(region) = region.as_deref_mut() {
            if region.remove(&old) {
              region.extend(new_faces);
            }
          }
        },
      );
      let p = out.vertices[*vm.get_or_insert(v)].position;
      out.vertices[v].position = p;
      vals.insert(v, [h, 0., 0., 0.]);
      twins.borrow_mut().entry(bits(p)).or_default().push(v);
      *creases.borrow_mut().verts.entry(bits(p)).or_default() |= 1 << i;
    }
    let vm = vm.unwrap();
    Ok(Crease::Split(vm, [a, b]))
  };

  // The creases edge a-b (with c and d the other corners of its faces) runs along, as bits. Both
  // ends on one crease aren't enough: a chord across a curved crease, or between two of its
  // branches, joins such vertices too. As `embed_path` requires its crease edges to run along
  // the guard's zero set, the faces on either side must lie on opposite sides of it.
  let along = |out: &LinkedMesh<()>,
               verts: &FxHashMap<[u32; 3], u16>,
               [a, b]: [VertexKey; 2],
               [c, d]: [VertexKey; 2]|
   -> u16 {
    let on = |k: VertexKey| {
      verts
        .get(&bits(out.vertices[k].position))
        .copied()
        .unwrap_or(0)
    };
    // An apex that is itself on the crease (another branch of it across a narrow strip, or the
    // next vertex along a curved one) says nothing about sides. The first is fine; the second
    // makes this edge a chord over a sliver, told apart by how flat that face is.
    let lifts = |k: VertexKey| {
      let [pa, pb, pk] = [a, b, k].map(|k| out.vertices[k].position);
      (pb - pa).cross(&(pk - pa)).norm() > 0.1 * (pb - pa).norm_squared()
    };
    let common = on(a) & on(b);
    (0..=FEATURE)
      .filter(|&i| {
        common & (1 << i) != 0
          && match (on(c) & (1 << i) != 0, on(d) & (1 << i) != 0) {
            (true, true) => false,
            (true, false) => lifts(c),
            (false, true) => lifts(d),
            (false, false) if i == FEATURE => feature(out, c) != feature(out, d),
            (false, false) => {
              let g = |k: VertexKey| guards_at(out, k).and_then(|g| g.get(i).copied());
              matches!((g(c), g(d)), (Some(gc), Some(gd)) if clear_straddle(gc, gd))
            }
          }
      })
      .fold(0, |bits, i| bits | 1 << i)
  };

  // Creases go onto edges before the tolerance refinement, during it, and after. Before, so a
  // height is smooth over each face from the start and a straight crease needs no refinement at
  // all. During (the refinement offers each face it finds off to `try_edge` first), because a
  // curved crease's coarse first round leaves chords, and bisecting toward the kink between a
  // chord and its arc never ends above `min_edge`. After, for whatever the new edges cross.
  for (round, sweep) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
    if (round, sweep) == (1, 0) {
      // Sweep 2: by the edge's ends, only for guards already known to mark real creases, and
      // each edge once. Anything more costs callback calls on every face the refinement visits.
      let mut tried: FxHashSet<EdgeKey> = FxHashSet::default();
      let mut hook = |out: &mut LinkedMesh<()>,
                      vals: &mut FxHashMap<VertexKey, Lanes>,
                      region: Region,
                      offer: Offer| {
        match offer {
          // Not seam edges: the refinement bisects those with their twins, which a split of one
          // side here would unpair.
          Offer::Edge(ek) if out.edges[ek].faces.len() == 2 && tried.insert(ek) => {
            try_edge(out, vals, region, ek, 2, None)
          }
          Offer::Edge(_) => Ok(Crease::None),
          // The midpoint of an edge that ran along a crease belongs on the crease: on its chord
          // it would stand just off a curved one.
          Offer::Bisected(vm, ends, apexes) => {
            let shared = along(out, &creases.borrow().verts, ends, apexes);
            if shared == 0 {
              return Ok(Crease::None);
            }
            for apex in apexes {
              let Some(ek) = out.get_edge_key([vm, apex]) else {
                continue;
              };
              if let slid @ Crease::Slid(_) = try_edge(out, vals, region, ek, 2, Some(vm))? {
                return Ok(slid);
              }
            }
            *creases
              .borrow_mut()
              .verts
              .entry(bits(out.vertices[vm].position))
              .or_default() |= shared;
            Ok(Crease::None)
          }
        }
      };
      refine_region(
        out,
        xf,
        vals,
        r,
        None,
        region.as_deref_mut(),
        Some(&mut hook),
        sample,
      )?;
    }
    // The second sweep costs a few callback calls per edge near a crease: worth it over the
    // region of an `emboss`, too slow over a whole displaced mesh. The halves of an edge it
    // splits go back on the list, since the rest of the edge may cross again.
    if sweep == 1 && (proven.get() == 0 || region.is_none()) {
      continue;
    }
    let mut by_pos = twins.borrow_mut();
    by_pos.clear();
    for (k, v) in out.vertices.iter() {
      by_pos.entry(bits(v.position)).or_default().push(k);
    }
    drop(by_pos);
    let mut edges: Vec<EdgeKey> = out.edges.keys().collect();
    while !edges.is_empty() {
      let mut split = false;
      while let Some(ek) = edges.pop() {
        if let Crease::Split(vm, ends) = try_edge(out, vals, &mut region, ek, sweep, None)? {
          split = true;
          if sweep == 1 {
            edges.extend(
              ends
                .into_iter()
                .filter_map(|end| out.get_edge_key([end, vm])),
            );
          }
        }
      }
      // Splitting a face re-keys its border edges, so a seam edge listed above may have gone
      // unvisited while its twin was split.
      if split && sweep == 0 {
        edges.extend(
          out
            .edges
            .iter()
            .filter(|(_, e)| e.faces.len() == 1)
            .map(|(k, _)| k),
        );
      }
    }
  }
  let mut creases = creases.into_inner();

  // Vertices that were already on a guard's zero (a crease along existing mesh edges) straddle
  // nothing; they count too.
  let mut largest = Guards::new();
  for g in at_vertex.borrow().values().flatten() {
    largest.resize(largest.len().max(g.len()), 0.);
    for (m, g) in largest.iter_mut().zip(g) {
      *m = m.max(g.abs());
    }
  }
  for (k, g) in at_vertex.borrow().iter() {
    let Some(v) = out.vertices.get(*k) else {
      continue;
    };
    for (i, (g, m)) in g.iter().flatten().zip(&largest).enumerate() {
      if g.abs() <= 1e-5 * m {
        *creases.verts.entry(bits(v.position)).or_default() |= 1 << i;
      }
    }
  }

  let mut found = Vec::new();
  for e in out.edges.values() {
    let [f0, f1] = e.faces[..] else {
      continue;
    };
    if !in_region(&region, &e.faces) {
      continue;
    }
    let apex = |f: FaceKey| {
      out.faces[f]
        .vertices
        .into_iter()
        .find(|k| !e.vertices.contains(k))
    };
    let (Some(c), Some(d)) = (apex(f0), apex(f1)) else {
      continue;
    };
    if along(out, &creases.verts, e.vertices, [c, d]) != 0 {
      let [pa, pb] = e.vertices.map(|k| bits(out.vertices[k].position));
      found.push([pa.min(pb), pa.max(pb)]);
    }
  }
  creases.edges.extend(found);
  Ok(creases)
}

/// Marks sharp those of `edges` whose faces actually meet at an angle: a guard's zero is only
/// where a height *may* crease.
fn mark_creased(m: &mut LinkedMesh<()>, edges: impl IntoIterator<Item = EdgeKey>) {
  const MIN_CREASE: f32 = 0.1;
  for e in edges {
    let [f0, f1] = m.edges[e].faces[..] else {
      continue;
    };
    let (n0, n1) = (
      m.faces[f0].normal(&m.vertices),
      m.faces[f1].normal(&m.vertices),
    );
    if n0.angle(&n1) > MIN_CREASE {
      m.edges[e].sharp = true;
    }
  }
}

/// Auto-smooth leaves a vertex whose faces are all zero-area without a normal; each takes a
/// neighbor's, so the set stays complete (which is what marks it authored) and splits can lerp it.
fn fill_missing_normals(m: &mut LinkedMesh<()>) {
  let mut missing: Vec<VertexKey> = m
    .vertices
    .keys()
    .filter(|&k| !m.shading_normals.contains_key(k))
    .collect();
  while !missing.is_empty() {
    let found: Vec<(VertexKey, Vec3)> = missing
      .iter()
      .filter_map(|&k| {
        let mut around = m.vertices[k]
          .edges
          .iter()
          .flat_map(|&e| m.edges[e].vertices);
        Some((k, *around.find_map(|n| m.shading_normals.get(n))?))
      })
      .collect();
    if found.is_empty() {
      m.shading_normals
        .extend(missing.drain(..).map(|k| (k, Vec3::y())));
    }
    m.shading_normals.extend(found);
    missing.retain(|&k| !m.shading_normals.contains_key(k));
  }
}

struct Offsets<'a> {
  /// Refined, undisplaced.
  mesh: LinkedMesh<()>,
  /// Per vertex: mitered offset direction and height.
  offsets: Vec<(VertexKey, Vec3, f32)>,
  height: Height<'a>,
  creases: Creases,
  /// Base creases were split so shading normals can be authored from the field.
  shade: bool,
}

fn height_offsets<'a>(
  ctx: &EvalCtx,
  builtin: &'static str,
  height_val: &'a Value,
  mesh: &MeshHandle,
  refine: &Option<Refine>,
  shade: bool,
  dist: Option<&Dist>,
) -> Result<Offsets<'a>, ErrorStack> {
  let mut out = (*mesh.mesh).clone();
  let shade = shade
    && matches!(height_val, Value::Callable(_))
    && !matches!(Height::new(ctx, builtin, height_val, &out)?, Height::Cb(cb) if cb.reads_vertex_data());
  // Shading normals come from the field rather than the displaced triangles, which streak on
  // sliver-ridden input (CSG output). Creases are split while the surface is still undisplaced,
  // so every vertex carries its side's base normal through refinement.
  if shade && out.shading_normals.len() != out.vertices.len() {
    out.mark_edge_sharpness(ctx.read_sharp_angle_threshold_degrees().to_radians());
    out.separate_vertices_and_compute_normals();
    fill_missing_normals(&mut out);
  }
  let height = Height::new(ctx, builtin, height_val, &out)?;
  let wrap_err = |err: ErrorStack| err.wrap(format!("error calling `{builtin}` height callback"));

  let mut vals: FxHashMap<VertexKey, Lanes> = FxHashMap::default();
  for group in coincident_groups(&out) {
    let n = unit(group_dir(&out, &group, |_| true));
    for &k in &group {
      let d = dist.map(|d| d.at_vertex(ctx, &out, k)).transpose()?;
      let h = height.at_vertex(ctx, &out, k, n, d).map_err(wrap_err)?;
      vals.insert(k, [h, 0., 0., 0.]);
    }
  }
  let mut creases = Creases::default();
  if let Some(r) = refine {
    if matches!(&height, Height::Cb(cb) if cb.reads_vertex_data()) {
      return Err(refine_needs_pos_only(builtin));
    }
    // Refined on the height alone: the mitered direction slides points sideways near creases,
    // which a comparison of offset vectors would chase with splits on a surface that is flat.
    creases = refine_height(
      ctx,
      &mut out,
      &mesh.transform,
      &height,
      height_val,
      &mut vals,
      r,
      None,
      dist,
    )?;
  }
  // Seam twins take their mean height, so fields that differ across a seam (anything read from
  // UVs) can't open it.
  let offsets = coincident_groups(&out)
    .iter()
    .flat_map(|group| {
      let h = group.iter().map(|k| vals[k][0]).sum::<f32>() / group.len() as f32;
      let dir = group_dir(&out, group, |_| true);
      group.iter().map(move |&k| (k, dir, h))
    })
    .collect();
  Ok(Offsets {
    mesh: out,
    offsets,
    height,
    creases,
    shade,
  })
}

/// First-order normals of `p + n·h(p)` at every vertex off the height's creases: the base
/// shading normal tilted against the height's surface gradient.
fn field_normals(ctx: &EvalCtx, o: &Offsets) -> Result<Vec<(VertexKey, Vec3)>, ErrorStack> {
  let out = &o.mesh;
  let aabb = out.compute_aabb(&Matrix4::identity());
  let min_eps = (aabb.maxs - aabb.mins).norm() * 2.5e-4;
  let mut normals = Vec::with_capacity(out.vertices.len());
  for (k, v) in out.vertices.iter() {
    let p = v.position;
    let Some(n) = out.shading_normals[k].try_normalize(1e-6) else {
      continue;
    };
    if o
      .creases
      .verts
      .contains_key(&[p.x.to_bits(), p.y.to_bits(), p.z.to_bits()])
    {
      continue;
    }
    let reach: f32 = v
      .edges
      .iter()
      .map(|&e| {
        let [a, b] = out.edges[e].vertices.map(|v| out.vertices[v].position);
        (a - b).norm()
      })
      .sum();
    // Central differences over half the local edge length: the slope the mesh can represent
    // (finer field detail would alias into speckle).
    let eps = (reach / v.edges.len().max(1) as f32 * 0.5).max(min_eps);
    let axis = if n.x.abs() < 0.9 {
      Vec3::x()
    } else {
      Vec3::y()
    };
    let t = n.cross(&axis).normalize();
    let mut tilted = n;
    for t in [t, n.cross(&t)] {
      let d = o.height.at(ctx, p + t * eps, n, None)? - o.height.at(ctx, p - t * eps, n, None)?;
      tilted -= t * (d / (2. * eps));
    }
    normals.push((k, tilted.normalize()));
  }
  Ok(normals)
}

fn displaced(
  ctx: &EvalCtx,
  height: &Value,
  mesh: &MeshHandle,
  refine: &Option<Refine>,
  split_seams: bool,
) -> Result<(LinkedMesh<()>, bool), ErrorStack> {
  let mut o = height_offsets(ctx, "displace", height, mesh, refine, split_seams, None)?;
  let normals = if o.shade {
    field_normals(ctx, &o).map_err(|err| err.wrap("error calling `displace` height callback"))?
  } else {
    Vec::new()
  };
  let mut out = std::mem::take(&mut o.mesh);
  // Welded output: the height's creases are known exactly, so they are marked instead of being
  // left to the auto-smooth threshold.
  let creased: Vec<EdgeKey> = if o.shade {
    Vec::new()
  } else {
    out
      .edges
      .iter()
      .filter(|(_, e)| o.creases.along(&out, e.vertices))
      .map(|(k, _)| k)
      .collect()
  };
  for &(k, dir, h) in &o.offsets {
    out.vertices[k].position += dir * h;
  }
  mark_creased(&mut out, creased);
  if !o.shade {
    return Ok((out, false));
  }
  if o.creases.verts.is_empty() {
    for (k, n) in normals {
      out.shading_normals[k] = n;
    }
  } else {
    // Vertices on the height's creases need a normal per side, which the auto-smooth pass
    // supplies (splitting them where the displaced faces meet sharply). It clones vertices, so
    // the field normals are put back by position; base-crease twins share one, and each takes
    // the candidate nearest its geometric normal.
    let bits = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut by_pos: FxHashMap<[u32; 3], SmallVec<[Vec3; 2]>> = FxHashMap::default();
    for (k, n) in normals {
      by_pos
        .entry(bits(out.vertices[k].position))
        .or_default()
        .push(n);
    }
    out.recompute_shading_normals_preserving_seams(
      ctx.read_sharp_angle_threshold_degrees().to_radians(),
    );
    let keys: Vec<VertexKey> = out.vertices.keys().collect();
    for k in keys {
      let Some(candidates) = by_pos.get(&bits(out.vertices[k].position)) else {
        continue;
      };
      let geometric = out.shading_normals.get(k).copied().unwrap_or_default();
      let best = candidates
        .iter()
        .max_by(|a, b| a.dot(&geometric).total_cmp(&b.dot(&geometric)));
      out.set_shading_normal(k, best.copied());
    }
    fill_missing_normals(&mut out);
  }
  out.flags |= mesh_flags::NO_WELD;
  Ok((out, true))
}

pub(crate) fn displace_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let refine = refine_kwargs(
    "displace",
    mesh,
    arg_refs[2].resolve(args, kwargs),
    arg_refs[3].resolve(args, kwargs),
    arg_refs[4].resolve(args, kwargs),
  )?;
  let height = arg_refs[0].resolve(args, kwargs);
  let split_seams = arg_refs[5].resolve(args, kwargs).as_bool().unwrap();
  let (out, shaded) = displaced(ctx, height, mesh, &refine, split_seams)?;
  Ok(if shaded {
    handle(mesh, out)
  } else {
    wrap(ctx, mesh, out)
  })
}

pub(crate) fn warp_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let warp_fn = arg_refs[0].resolve(args, kwargs).as_callable().unwrap();
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let refine = refine_kwargs(
    "warp",
    mesh,
    arg_refs[2].resolve(args, kwargs),
    arg_refs[3].resolve(args, kwargs),
    arg_refs[4].resolve(args, kwargs),
  )?;
  let mut out = (*mesh.mesh).clone();
  let cb = VertexCb::new(ctx, "warp", warp_fn, &["pos", "normal"], &out)?;
  if refine.is_some() && cb.reads_vertex_data() {
    return Err(refine_needs_pos_only("warp"));
  }
  if cb.reads_fixed(1)
    && out
      .vertices
      .keys()
      .next()
      .is_some_and(|k| out.displacement_normal(k).is_none())
  {
    out.compute_vertex_displacement_normals();
  }
  let pos = |v: Value| -> Result<Lanes, ErrorStack> {
    let p = v
      .as_vec3()
      .ok_or_else(|| ErrorStack::new(format!("warp callback must return Vec3, got: {v:?}")))?;
    Ok([p.x, p.y, p.z, 0.])
  };
  let wrap_err = |err: ErrorStack| err.wrap("error calling warp cb");

  let mut vals: FxHashMap<VertexKey, Lanes> = FxHashMap::default();
  for (k, v) in out.vertices.iter() {
    let normal = out.displacement_normal(k).unwrap_or_default();
    let warped = cb
      .invoke(
        ctx,
        &[Value::Vec3(v.position), Value::Vec3(normal)],
        &out,
        k,
      )
      .map_err(wrap_err)?;
    vals.insert(k, pos(warped)?);
  }
  // Positions are linear across a face, so a warped probe off the corners' lerp is exactly the
  // warp's deviation from the flat triangle.
  if let Some(r) = &refine {
    refine_field(
      &mut out,
      &mesh.transform,
      &mut vals,
      r,
      None,
      |m, p, site| {
        pos(
          cb.invoke_fixed(ctx, &[Value::Vec3(p), Value::Vec3(site.normal(m))])
            .map_err(wrap_err)?,
        )
      },
    )?;
  }
  for (k, v) in out.vertices.iter_mut() {
    let [x, y, z, _] = vals[&k];
    v.position = Vec3::new(x, y, z);
  }
  Ok(wrap(ctx, mesh, out))
}

fn fill_channel(m: &mut LinkedMesh<()>, name: &str, value: f32) {
  let mut ch = Channel::for_attr(name, Arity::Scalar);
  for k in m.vertices.keys() {
    ch.set(k, [value, 0., 0., 0.]);
  }
  m.vertex_channels.insert(name.to_owned(), ch);
}

/// Closes an open patch into the solid it sweeps between two per-vertex offsets, `top` being
/// the one further along the surface normal. The patch's own faces become the top.
#[cfg(target_arch = "wasm32")]
fn prism(patch: &LinkedMesh<()>, ends: impl Fn(VertexKey) -> (Vec3, Vec3)) -> LinkedMesh<()> {
  let mut out = patch.clone();
  out.shading_normals.clear();
  out.displacement_normals.clear();
  out.edge_displacement_normals.clear();
  let rim: Vec<[VertexKey; 2]> = out
    .edges
    .values()
    .filter(|e| e.faces.len() == 1)
    .map(|e| {
      let vs = out.faces[e.faces[0]].vertices;
      let i = (0..3)
        .find(|&i| e.vertices.contains(&vs[i]) && e.vertices.contains(&vs[(i + 1) % 3]))
        .unwrap();
      [vs[i], vs[(i + 1) % 3]]
    })
    .collect();
  // Seams inside the patch (authored ones, and earlier cut curves, which booleans keep as
  // duplicate vertices per operand) are open edges too, each facing its coincident twin. They
  // get no wall; Manifold closes them through the mesh's merge vectors.
  let ends_of = |[a, b]: [VertexKey; 2]| [a, b].map(|k| out.vertices[k].position.map(f32::to_bits));
  let open: FxHashSet<[_; 2]> = rim.iter().map(|&e| ends_of(e)).collect();
  let rim: Vec<[VertexKey; 2]> = rim
    .into_iter()
    .filter(|&[a, b]| !open.contains(&ends_of([b, a])))
    .collect();
  out.flags |= mesh_flags::NO_WELD;
  let tops: Vec<[VertexKey; 3]> = out.faces.values().map(|f| f.vertices).collect();
  let keys: Vec<VertexKey> = out.vertices.keys().collect();
  let mut below: FxHashMap<VertexKey, VertexKey> = FxHashMap::default();
  for k in keys {
    let (top, bottom) = ends(k);
    let base = out.vertices[k].position;
    out.vertices[k].position = base + top;
    let k2 = out
      .vertices
      .insert(mesh::linked_mesh::Vertex::new(base + bottom));
    for ch in out.vertex_channels.values_mut() {
      if let Some(v) = ch.get(k) {
        ch.set(k2, v);
      }
    }
    below.insert(k, k2);
  }
  for [a, b, c] in tops {
    out.add_face::<false>([below[&c], below[&b], below[&a]], ());
  }
  for [a, b] in rim {
    out.add_face::<false>([below[&a], below[&b], b], ());
    out.add_face::<false>([below[&a], b, a], ());
  }
  out
}

/// `emboss` for a mask solid over a solid. Manifold finds the region (the surface inside the
/// mask, bounded by the exact intersection curve); that patch alone is swept along its own
/// mitered normals into a prism, which is then unioned on (raised parts) or subtracted
/// (sunk parts), so the rest of the mesh keeps its triangulation and overlapping raised
/// regions merge. `None` when a mesh isn't a closed manifold or the patch pinches at a vertex.
#[cfg(target_arch = "wasm32")]
fn emboss_solid(
  ctx: &EvalCtx,
  mask: &MeshHandle,
  height: &Value,
  mesh: &MeshHandle,
  refine: &Option<Refine>,
  transformed: bool,
  tag: Option<&str>,
) -> Result<Option<Value>, ErrorStack> {
  use crate::mesh_ops::{
    mesh_boolean::{apply_mesh_boolean_op, MeshBooleanOp::*},
    split_field::cut_contour,
  };

  const MARK: &str = "__emboss_mask";
  let handle = |mesh, transform| MeshHandle {
    mesh,
    transform,
    manifold_handle: Rc::new(ManifoldHandle::new_empty()),
    aabb: Default::default(),
    bvh: Default::default(),
    material: None,
  };
  let xf = if transformed {
    mesh.transform
  } else {
    Matrix4::identity()
  };
  let base = MeshHandle {
    manifold_handle: Rc::clone(&mesh.manifold_handle),
    ..handle(Rc::clone(&mesh.mesh), xf)
  };
  if base.get_or_create_handle().is_err() || mask.get_or_create_handle().is_err() {
    return Ok(None);
  }

  // Faces keep their operand's property vertices through a boolean, so a lane that is 1 on the
  // mask and absent (0) on the mesh tells the two apart in the intersection.
  let mut marked = (*mask.mesh).clone();
  fill_channel(&mut marked, MARK, 1.);
  let inside = apply_mesh_boolean_op(
    &base,
    &handle(Rc::new(marked), mask.transform),
    Intersection,
    false,
    true,
  )?;
  let keep = |m: &mut LinkedMesh<()>, keep: &dyn Fn(&LinkedMesh<()>, [VertexKey; 3]) -> bool| {
    let drop: Vec<_> = m
      .faces
      .iter()
      .filter(|(_, f)| !keep(m, f.vertices))
      .map(|(k, _)| k)
      .collect();
    for f in drop {
      m.remove_face(f);
    }
    let loose: Vec<VertexKey> = m
      .vertices
      .iter()
      .filter(|(_, v)| v.edges.is_empty())
      .map(|(k, _)| k)
      .collect();
    for k in loose {
      m.remove_vertex(k);
    }
  };
  let mut patch = (*inside.mesh).clone();
  let mark = patch.vertex_channels.remove(MARK).unwrap();
  keep(&mut patch, &|_, vs| {
    vs.iter().all(|&k| mark.get(k).is_none_or(|v| v[0] < 0.5))
  });

  let patch_xf = if transformed {
    Matrix4::identity()
  } else {
    mesh.transform
  };
  let Offsets {
    mesh: mut patch,
    offsets,
    ..
  } = height_offsets(
    ctx,
    "emboss",
    height,
    &handle(Rc::new(patch), patch_xf),
    refine,
    false,
    Some(&Dist::Mask(mask.get_or_create_bvh())),
  )?;
  // Cut along the height's zero contour, so a height that changes sign over the region gives
  // a raised prism and a sunk one that meet edge to edge.
  let mut heights: FxHashMap<VertexKey, f32> = offsets.iter().map(|o| (o.0, o.2)).collect();
  cut_contour(&mut patch, &mut heights, 0.);
  if let Some(tag) = tag {
    fill_channel(&mut patch, attr_name(tag)?, 1.);
  }
  // The cap on the surface side sits this far past it, leaving nothing coplanar with the
  // surface for the boolean to resolve.
  let skin = heights.values().fold(0f32, |m, h| m.max(h.abs())) * 1e-3;

  let mut out = base;
  for (sign, op) in [(1., Union), (-1., Difference)] {
    let mut part = patch.clone();
    keep(&mut part, &|_, vs| {
      vs.iter().any(|k| heights[k] * sign > 0.)
    });
    if part.faces.is_empty() {
      continue;
    }
    let dirs: FxHashMap<VertexKey, Vec3> = coincident_groups(&part)
      .iter()
      .flat_map(|group| {
        let dir = group_dir(&part, group, |_| true);
        group.iter().map(move |&k| (k, dir))
      })
      .collect();
    let solid = prism(&part, |k| {
      let (far, near) = (dirs[&k] * heights[&k], dirs[&k] * (-sign * skin));
      if sign > 0. {
        (far, near)
      } else {
        (near, far)
      }
    });
    out = apply_mesh_boolean_op(
      &out,
      &handle(Rc::new(solid), Matrix4::identity()),
      op,
      false,
      false,
    )?;
  }
  if !transformed {
    out.transform = mesh.transform;
  }
  out.material = mesh.material.clone();
  Ok(Some(Value::Mesh(Rc::new(out))))
}

pub(crate) fn emboss_impl(
  ctx: &EvalCtx,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let field = arg_refs[0].resolve(args, kwargs);
  let height = arg_refs[1].resolve(args, kwargs);
  let mesh = arg_refs[2].resolve(args, kwargs).as_mesh().unwrap();
  let thr = arg_refs[3].resolve(args, kwargs).as_float().unwrap();
  let refine = refine_kwargs(
    "emboss",
    mesh,
    arg_refs[4].resolve(args, kwargs),
    arg_refs[5].resolve(args, kwargs),
    arg_refs[6].resolve(args, kwargs),
  )?;
  let transformed = arg_refs[7].resolve(args, kwargs).as_bool().unwrap();
  let tag = arg_refs[8].resolve(args, kwargs).as_str();
  #[cfg(target_arch = "wasm32")]
  if let (Value::Mesh(mask), true) = (field, thr == 0.) {
    if let Some(out) = emboss_solid(ctx, mask, height, mesh, &refine, transformed, tag)? {
      return Ok(out);
    }
  }
  let chart = arg_refs[9].resolve(args, kwargs).as_str().unwrap();
  let (mut out, field, src) =
    cut_by_field(ctx, "emboss", field, mesh, thr, refine, transformed, chart)?;
  let height_cb = Height::new(ctx, "emboss", height, &out)?;
  let wrap_err = |err: ErrorStack| err.wrap("error calling `emboss` height callback");
  let mut region: FxHashSet<FaceKey> = out
    .faces
    .iter()
    .filter(|(_, f)| f.vertices.iter().any(|k| field[k] < thr))
    .map(|(k, _)| k)
    .collect();
  let mut vals: FxHashMap<VertexKey, Lanes> = FxHashMap::default();
  for &f in &region {
    for k in out.faces[f].vertices {
      if vals.contains_key(&k) {
        continue;
      }
      let n = unit(group_dir(&out, &[k], |f| region.contains(&f)));
      let d = field[&k] - thr;
      let h = height_cb
        .at_vertex(ctx, &out, k, n, Some(d))
        .map_err(wrap_err)?;
      vals.insert(k, [h, 0., 0., 0.]);
    }
  }
  let free = matches!(&height_cb, Height::Cb(cb) if !cb.reads_vertex_data());
  let dist = Dist::Field(&src, thr);
  let dist = src.probes_freely().then_some(&dist);
  let mut creases = Creases::default();
  if let Some(r) = refine.filter(|_| free) {
    creases = refine_height(
      ctx,
      &mut out,
      &mesh.transform,
      &height_cb,
      height,
      &mut vals,
      &r,
      Some(&mut region),
      dist,
    )
    .map_err(wrap_err)?;
  }
  // Which sides of the region's faces run along a crease of the height. Per face because
  // extrusion clones the outline's vertices but keeps faces.
  let on_crease: FxHashMap<FaceKey, [bool; 3]> = region
    .iter()
    .map(|&f| {
      let vs = out.faces[f].vertices;
      (
        f,
        [0, 1, 2].map(|i| creases.along(&out, [vs[i], vs[(i + 1) % 3]])),
      )
    })
    .collect();
  let raised = extrude_region(
    &mut out,
    |_, f| region.contains(&f),
    |m, k, n| match vals.get(&k) {
      Some(v) => Ok(v[0]),
      // a seam twin with no face of its own in the region
      None => height_cb
        .at_vertex(ctx, m, k, n, Some(field.get(&k).map_or(0., |f| f - thr)))
        .map_err(wrap_err),
    },
  )?;
  if free {
    // The region's outline and the height's creases are known exactly, so they are marked
    // rather than left to the auto-smooth threshold, which a 40° chamfer falls under.
    let mut creased = Vec::new();
    for (f, on_crease) in &on_crease {
      let face = &out.faces[*f];
      for i in 0..3 {
        let ends = [face.vertices[i], face.vertices[(i + 1) % 3]];
        let Some(e) = out.get_edge_key(ends) else {
          continue;
        };
        if out.edges[e].faces.iter().any(|g| !raised.contains(g)) {
          out.edges[e].sharp = true;
        } else if on_crease[i] {
          creased.push(e);
        }
      }
    }
    mark_creased(&mut out, creased);
  }
  if let Some(tag) = tag {
    let name = attr_name(tag)?;
    fill_channel(&mut out, name, 0.);
    let ch = out.vertex_channels.get_mut(name).unwrap();
    for f in raised {
      for k in out.faces[f].vertices {
        ch.set(k, [1., 0., 0., 0.]);
      }
    }
  }
  Ok(wrap(ctx, mesh, out))
}

pub(crate) fn surface_chart_impl(
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let origin = *arg_refs[0].resolve(args, kwargs).as_vec3().unwrap();
  let mesh = arg_refs[1].resolve(args, kwargs).as_mesh().unwrap();
  let up = *arg_refs[2].resolve(args, kwargs).as_vec3().unwrap();
  let name = attr_name(arg_refs[3].resolve(args, kwargs).as_str().unwrap())?;
  let mut out = (*mesh.mesh).clone();
  let mut ch = Channel::for_attr(name, Arity::Vec2);
  for (k, uv) in surface_chart(&out, origin, up) {
    ch.set(k, [uv.x, uv.y, 0., 0.]);
  }
  out.vertex_channels.insert(name.to_owned(), ch);
  Ok(Value::Mesh(Rc::new(MeshHandle {
    mesh: Rc::new(out),
    transform: mesh.transform,
    manifold_handle: Rc::new(ManifoldHandle::new_empty()),
    aabb: Default::default(),
    bvh: Default::default(),
    material: mesh.material.clone(),
  })))
}
