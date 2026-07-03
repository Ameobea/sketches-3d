//! Harmonic tube unwrap for closed genus-0 tube-like meshes (bent/deformed pipes, trim, extrusions).
//! U wraps the cross-section once (like `cylindrical`); V runs along the tube, arc-length uniform.
//! Method: cotan-Laplacian harmonic field between the two Fiedler-detected ends -> Dijkstra cut ->
//! slit to disk -> second harmonic solve between the slit banks.  Caps (crease-bounded transverse
//! terminal patches) become separate planar islands at body texel density.

use std::{cell::RefCell, cmp::Reverse, collections::BinaryHeap, rc::Rc};

use faer::linalg::solvers::Solve;
use faer::sparse::linalg::solvers::{Llt, SymbolicLlt};
use faer::sparse::{SparseColMat, Triplet};
use faer::{Mat, Side};
use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{mesh_flags, EdgeKey, FaceKey, Vec3, VertexKey},
  slotmap_utils::vkey,
  LinkedMesh,
};
use nalgebra::Vector3;

use super::compute_uvs::{new_tangent_channel, new_uv_channel, orthonormal_basis};
use crate::{ErrorStack, ManifoldHandle, MeshHandle, Value};

type V3 = Vector3<f64>;

pub(crate) struct TubeOptions {
  caps: bool,
  /// Crease angle bounding cap growth; defaults to the ctx sharp-edge threshold.
  cap_angle_rad: Option<f32>,
  /// Max fraction of total spine arc length a cap patch may span.
  cap_max_span: f64,
  /// Min |cap normal . local end tangent| for a patch to count as transverse.
  cap_alignment: f64,
  /// V spans 0..1 over the tube length instead of isotropic (arc/perimeter) scaling.
  normalize_v: bool,
  /// Penalty weight steering the seam cut longitudinally (0 = pure shortest path).
  seam_straightness: f64,
  /// Cancel rotational drift of U along the spine (RMF-referenced phase correction).
  detwist: bool,
}

fn parse_tube_options(options: Option<&FxHashMap<String, Value>>) -> Result<TubeOptions, ErrorStack> {
  let mut out = TubeOptions {
    caps: true,
    cap_angle_rad: None,
    cap_max_span: 0.15,
    cap_alignment: 0.6,
    normalize_v: false,
    seam_straightness: 8.,
    detwist: true,
  };
  let Some(map) = options else { return Ok(out) };
  for (key, val) in map {
    match key.as_str() {
      "caps" => {
        out.caps = match (val.as_str(), val.as_bool()) {
          (Some("auto"), _) => true,
          (Some("none"), _) => false,
          (None, Some(b)) => b,
          _ => {
            return Err(ErrorStack::new(
              "`compute_uvs` option `caps` must be 'auto', 'none', or a boolean",
            ))
          }
        }
      }
      "cap_angle" => {
        let deg = val
          .as_float()
          .ok_or_else(|| ErrorStack::new("`compute_uvs` option `cap_angle` must be a number (degrees)"))?;
        out.cap_angle_rad = Some(deg.to_radians());
      }
      "cap_max_span" => {
        out.cap_max_span = val
          .as_float()
          .ok_or_else(|| ErrorStack::new("`compute_uvs` option `cap_max_span` must be a number"))?
          as f64;
      }
      "cap_alignment" => {
        out.cap_alignment = val
          .as_float()
          .ok_or_else(|| ErrorStack::new("`compute_uvs` option `cap_alignment` must be a number"))?
          as f64;
      }
      "normalize_v" => {
        out.normalize_v = val
          .as_bool()
          .ok_or_else(|| ErrorStack::new("`compute_uvs` option `normalize_v` must be a boolean"))?;
      }
      "seam_straightness" => {
        out.seam_straightness = val
          .as_float()
          .ok_or_else(|| ErrorStack::new("`compute_uvs` option `seam_straightness` must be a number"))?
          as f64;
      }
      "detwist" => {
        out.detwist = val
          .as_bool()
          .ok_or_else(|| ErrorStack::new("`compute_uvs` option `detwist` must be a boolean"))?;
      }
      _ => {
        return Err(ErrorStack::new(format!(
          "Unknown option {key:?} for `compute_uvs(type='tube')`; supported options: `caps`, \
           `cap_angle`, `cap_max_span`, `cap_alignment`, `normalize_v`, `seam_straightness`, \
           `detwist`"
        )))
      }
    }
  }
  Ok(out)
}

/// Symmetric cotan-Laplacian triplets (both off-diag orientations + diagonal).  Cot weights are
/// clamped so degenerate CSG slivers can't inject infinities.
fn cotan_triplets(pos: &[V3], tris: &[[usize; 3]]) -> Vec<(usize, usize, f64)> {
  let mut trips = Vec::with_capacity(tris.len() * 12);
  for &[i, j, k] in tris {
    for &(a, b, c) in &[(i, j, k), (j, k, i), (k, i, j)] {
      let (e1, e2) = (pos[b] - pos[a], pos[c] - pos[a]);
      let w = (0.5 * e1.dot(&e2) / e1.cross(&e2).norm().max(1e-12)).clamp(-1e4, 1e4);
      trips.push((b, c, -w));
      trips.push((c, b, -w));
      trips.push((b, b, w));
      trips.push((c, c, w));
    }
  }
  trips
}

fn sparse_llt(n: usize, lower: &[Triplet<usize, usize, f64>]) -> Result<Llt<usize, f64>, ErrorStack> {
  let a = SparseColMat::<usize, f64>::try_new_from_triplets(n, n, lower)
    .map_err(|e| ErrorStack::new(format!("`compute_uvs`: failed to assemble Laplacian: {e:?}")))?;
  let sym = SymbolicLlt::try_new(a.symbolic(), Side::Lower)
    .map_err(|e| ErrorStack::new(format!("`compute_uvs`: symbolic factorization failed: {e:?}")))?;
  Llt::try_new_with_symbolic(sym, a.as_ref(), Side::Lower).map_err(|e| {
    ErrorStack::new(format!(
      "`compute_uvs`: Cholesky factorization failed (degenerate mesh?): {e:?}"
    ))
  })
}

/// Harmonic interpolation: solve L u = 0 with `pins` as Dirichlet boundary values.  Pinned DOFs are
/// eliminated so the reduced system stays SPD.
fn harmonic_interp(
  n: usize,
  l_full: &[(usize, usize, f64)],
  pins: &[(usize, f64)],
) -> Result<Vec<f64>, ErrorStack> {
  let mut pinned = vec![f64::NAN; n];
  for &(v, val) in pins {
    pinned[v] = val;
  }
  let mut fi = vec![usize::MAX; n];
  let mut nf = 0usize;
  for i in 0..n {
    if pinned[i].is_nan() {
      fi[i] = nf;
      nf += 1;
    }
  }
  let mut lower = Vec::with_capacity(l_full.len() / 2);
  let mut rhs = vec![0f64; nf];
  for &(r, c, v) in l_full {
    if !pinned[r].is_nan() {
      continue;
    }
    if pinned[c].is_nan() {
      if fi[r] >= fi[c] {
        lower.push(Triplet::new(fi[r], fi[c], v));
      }
    } else {
      rhs[fi[r]] -= v * pinned[c];
    }
  }
  let llt = sparse_llt(nf, &lower)?;
  let mut b = Mat::<f64>::zeros(nf, 1);
  b.col_as_slice_mut(0).copy_from_slice(&rhs);
  llt.solve_in_place(&mut b);
  let sol = b.col_as_slice(0);
  Ok(
    (0..n)
      .map(|i| if pinned[i].is_nan() { sol[fi[i]] } else { pinned[i] })
      .collect(),
  )
}

/// Fiedler vector (2nd-smallest Laplacian eigenvector) via inverse iteration on L + sigma*I with the
/// constant nullspace deflated each step.  Its argmin/argmax are the tube's two ends.
fn fiedler_ends(n: usize, l_full: &[(usize, usize, f64)], pos: &[V3]) -> Result<(usize, usize), ErrorStack> {
  let mut diag_mean = 0f64;
  for &(r, c, v) in l_full {
    if r == c {
      diag_mean += v;
    }
  }
  diag_mean /= n as f64;
  let sigma = (diag_mean * 1e-6).max(1e-12);
  let mut lower: Vec<Triplet<usize, usize, f64>> = l_full
    .iter()
    .filter(|&&(r, c, _)| r >= c)
    .map(|&(r, c, v)| Triplet::new(r, c, v))
    .collect();
  for i in 0..n {
    lower.push(Triplet::new(i, i, sigma));
  }
  let llt = sparse_llt(n, &lower)?;

  let mut x = Mat::<f64>::zeros(n, 1);
  for i in 0..n {
    x[(i, 0)] = pos[i].x + 0.7 * pos[i].y + 0.43 * pos[i].z + 1e-3 * (i % 17) as f64;
  }
  for _ in 0..48 {
    let mean = x.col_as_slice(0).iter().sum::<f64>() / n as f64;
    let mut norm = 0f64;
    for i in 0..n {
      x[(i, 0)] -= mean;
      norm += x[(i, 0)] * x[(i, 0)];
    }
    let norm = norm.sqrt().max(1e-30);
    for i in 0..n {
      x[(i, 0)] /= norm;
    }
    llt.solve_in_place(&mut x);
  }
  let xs = x.col_as_slice(0);
  let (mut lo, mut hi) = (0usize, 0usize);
  for i in 1..n {
    if xs[i] < xs[lo] {
      lo = i;
    }
    if xs[i] > xs[hi] {
      hi = i;
    }
  }
  if lo == hi {
    return Err(ErrorStack::new("`compute_uvs(type='tube')`: end detection failed (degenerate mesh?)"));
  }
  Ok((lo, hi))
}

fn neighbor(m: &LinkedMesh<()>, e: EdgeKey, v: VertexKey) -> VertexKey {
  let vs = m.edges[e].vertices;
  if vs[0] == v {
    vs[1]
  } else {
    vs[0]
  }
}

/// Cumulative arc length along iso-`along` bin centroids, as a 64-entry lookup (normalized 0..1)
/// plus the total spine length.  Corrects for harmonic compression near the pinned tips.
const ARC_BINS: usize = 64;
fn arc_reparam(pos: &[V3], along: &[f64]) -> (Vec<f64>, f64) {
  let (mut sum, mut cnt) = (vec![V3::zeros(); ARC_BINS], vec![0usize; ARC_BINS]);
  for i in 0..pos.len() {
    let b = (along[i].clamp(0., 1.) * (ARC_BINS as f64 - 1.)).round() as usize;
    sum[b] += pos[i];
    cnt[b] += 1;
  }
  let mut arc = vec![0f64; ARC_BINS];
  let (mut acc, mut prev): (f64, Option<V3>) = (0., None);
  for b in 0..ARC_BINS {
    if cnt[b] > 0 {
      let c = sum[b] / cnt[b] as f64;
      if let Some(p) = prev {
        acc += (c - p).norm();
      }
      prev = Some(c);
    }
    arc[b] = acc;
  }
  let total = acc.max(1e-9);
  (arc.iter().map(|a| a / total).collect(), total)
}

fn arc_lookup(table: &[f64], u: f64) -> f64 {
  let x = u.clamp(0., 1.) * (ARC_BINS as f64 - 1.);
  let (b, frac) = (x.floor() as usize, x.fract());
  if b + 1 < ARC_BINS {
    table[b] * (1. - frac) + table[b + 1] * frac
  } else {
    table[b]
  }
}

fn dijkstra_path(
  m: &LinkedMesh<()>,
  src: VertexKey,
  dst: VertexKey,
  edge_cost: impl Fn(VertexKey, VertexKey, f64) -> f64,
) -> Vec<VertexKey> {
  let keys: Vec<VertexKey> = m.vertices.iter().map(|(k, _)| k).collect();
  let ki: FxHashMap<VertexKey, usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
  let n = keys.len();
  let mut dist = vec![f64::INFINITY; n];
  let mut prev = vec![usize::MAX; n];
  let mut heap: BinaryHeap<Reverse<(u64, usize)>> = BinaryHeap::new();
  dist[ki[&src]] = 0.;
  heap.push(Reverse((0, ki[&src])));
  while let Some(Reverse((d_bits, u))) = heap.pop() {
    if f64::from_bits(d_bits) > dist[u] {
      continue;
    }
    if keys[u] == dst {
      break;
    }
    let pu = m.vertices[keys[u]].position;
    for &e in &m.vertices[keys[u]].edges {
      let nbk = neighbor(m, e, keys[u]);
      let nb = ki[&nbk];
      let len = (m.vertices[nbk].position - pu).norm() as f64;
      let nd = dist[u] + edge_cost(keys[u], nbk, len);
      if nd < dist[nb] {
        dist[nb] = nd;
        prev[nb] = u;
        heap.push(Reverse((nd.to_bits(), nb)));
      }
    }
  }
  let mut path = vec![dst];
  let mut cur = ki[&dst];
  while prev[cur] != usize::MAX {
    cur = prev[cur];
    path.push(keys[cur]);
  }
  path.reverse();
  path
}

/// Face on the side of directed edge a->b where (a,b) are consecutive in CCW winding.
fn left_face(m: &LinkedMesh<()>, a: VertexKey, b: VertexKey) -> Option<FaceKey> {
  let e = m.get_edge_key([a, b])?;
  for &fk in &m.edges[e].faces {
    let vs = m.faces[fk].vertices;
    for i in 0..3 {
      if vs[i] == a && vs[(i + 1) % 3] == b {
        return Some(fk);
      }
    }
  }
  None
}

/// Faces in the fan around `c` from `e_start` (beginning at `start_face`) up to `e_stop`.
fn fan_arc(m: &LinkedMesh<()>, c: VertexKey, e_start: EdgeKey, start_face: FaceKey, e_stop: EdgeKey) -> Vec<FaceKey> {
  let mut arc = Vec::new();
  let (mut cur_edge, mut cur_face) = (e_start, start_face);
  loop {
    arc.push(cur_face);
    let next = m.faces[cur_face]
      .edges
      .iter()
      .copied()
      .find(|&e| e != cur_edge && m.edges[e].vertices.contains(&c));
    let Some(next) = next else { break };
    if next == e_stop {
      break;
    }
    let faces = &m.edges[next].faces;
    if faces.len() < 2 {
      break;
    }
    cur_face = if faces[0] == cur_face { faces[1] } else { faces[0] };
    cur_edge = next;
  }
  arc
}

pub(crate) fn tube_uvs(
  mesh: &MeshHandle,
  scale: f32,
  sharp_threshold_rad: f32,
  options: Option<&FxHashMap<String, Value>>,
) -> Result<MeshHandle, ErrorStack> {
  let opts = parse_tube_options(options)?;
  let mut m = (*mesh.mesh).clone();

  if m.vertices.len() < 4 {
    return Err(ErrorStack::new("`compute_uvs(type='tube')`: mesh has too few vertices"));
  }
  let euler = m.vertices.len() as i64 - m.edges.len() as i64 + m.faces.len() as i64;
  if euler != 2 {
    return Err(ErrorStack::new(format!(
      "`compute_uvs(type='tube')` requires a closed genus-0 mesh (capped tube); got Euler \
       characteristic {euler} (2 = closed genus-0, 0 = open tube or torus)"
    )));
  }

  let keys: Vec<VertexKey> = m.vertices.iter().map(|(k, _)| k).collect();
  let k2i: FxHashMap<VertexKey, usize> = keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
  let pos: Vec<V3> = keys
    .iter()
    .map(|&k| {
      let p = m.vertices[k].position;
      V3::new(p.x as f64, p.y as f64, p.z as f64)
    })
    .collect();
  let tris: Vec<[usize; 3]> = m
    .faces
    .iter()
    .map(|(_, f)| [k2i[&f.vertices[0]], k2i[&f.vertices[1]], k2i[&f.vertices[2]]])
    .collect();

  // U (along-tube harmonic): Fiedler ends pinned 0/1
  let l_full = cotan_triplets(&pos, &tris);
  let (end_lo, end_hi) = fiedler_ends(pos.len(), &l_full, &pos)?;
  let along0 = harmonic_interp(pos.len(), &l_full, &[(end_lo, 0.), (end_hi, 1.)])?;

  // cut tip-to-tip, slit the tube into a disk; the two banks become the around-seam.  Azimuthal
  // (non-longitudinal) edge travel is penalized so the seam doesn't spiral around the tube toward
  // a shorter side — a spiraling seam drags the pinned U=0 meridian with it and the whole texture
  // twists along the spine.  Alignment is normalized per-vertex (an edge is longitudinal if its
  // |Δalong| is the largest among its endpoints' edges) so harmonic compression near the tips
  // can't zero out the signal exactly where the spiral happens.
  let (ak, bk) = (keys[end_lo], keys[end_hi]);
  let gmax: FxHashMap<VertexKey, f64> = keys
    .iter()
    .map(|&k| {
      let a0 = along0[k2i[&k]];
      let g = m.vertices[k]
        .edges
        .iter()
        .map(|&e| (along0[k2i[&neighbor(&m, e, k)]] - a0).abs())
        .fold(0f64, f64::max);
      (k, g.max(1e-30))
    })
    .collect();
  let lambda = opts.seam_straightness.max(0.);
  let cutpath = dijkstra_path(&m, ak, bk, |a, b, len| {
    let align = ((along0[k2i[&a]] - along0[k2i[&b]]).abs() / gmax[&a].max(gmax[&b])).min(1.);
    len * (1. + lambda * (1. - align))
  });
  if cutpath.len() < 2 || cutpath[0] != ak {
    return Err(ErrorStack::new(
      "`compute_uvs(type='tube')`: mesh is not connected (no path between detected ends)",
    ));
  }
  let mut moved: Vec<(VertexKey, Vec<FaceKey>)> = Vec::new();
  for w in cutpath.windows(3) {
    let (p, c, q) = (w[0], w[1], w[2]);
    let (Some(e_p), Some(e_q)) = (m.get_edge_key([c, p]), m.get_edge_key([c, q])) else {
      continue;
    };
    let Some(start) = left_face(&m, q, c) else { continue };
    moved.push((c, fan_arc(&m, c, e_q, start, e_p)));
  }
  let mut banks: Vec<(VertexKey, VertexKey)> = Vec::new();
  for (c, faces) in moved {
    let clone = m.split_off_faces(c, &faces);
    banks.push((c, clone));
  }
  if banks.is_empty() {
    return Err(ErrorStack::new(
      "`compute_uvs(type='tube')`: cut path too short to slit (mesh too coarse along the tube)",
    ));
  }

  // Split each tip vertex in two.  The seam terminates at the tips, so a single tip vertex carries
  // one shared `around` value into wall faces on BOTH sides of the seam, making them bridge the 0/1
  // wrap.  The slit already opened the tip's fan into a boundary-to-boundary strip (from the clone-
  // bank edge around to the orig-bank edge); cutting it at its middle — which lands inside the cap
  // on a capped tube — gives each side a copy the harmonic solve pulls toward that side's values.
  let mut tip_clones: Vec<(VertexKey, VertexKey)> = Vec::new();
  for (tip, nb) in [(ak, cutpath[1]), (bk, cutpath[cutpath.len() - 2])] {
    let Some(&(_, nb_clone)) = banks.iter().find(|&&(o, _)| o == nb) else {
      continue;
    };
    let (Some(e_o), Some(e_c)) = (m.get_edge_key([tip, nb]), m.get_edge_key([tip, nb_clone])) else {
      continue;
    };
    let Some(&start) = m.edges[e_c].faces.first() else { continue };
    let fan = fan_arc(&m, tip, e_c, start, e_o);
    if fan.len() >= 4 {
      tip_clones.push((tip, m.split_off_faces(tip, &fan[..fan.len() / 2])));
    }
  }

  // slit-mesh index space
  let keys2: Vec<VertexKey> = m.vertices.iter().map(|(k, _)| k).collect();
  let k2i2: FxHashMap<VertexKey, usize> = keys2.iter().enumerate().map(|(i, &k)| (k, i)).collect();
  let pos2: Vec<V3> = keys2
    .iter()
    .map(|&k| {
      let p = m.vertices[k].position;
      V3::new(p.x as f64, p.y as f64, p.z as f64)
    })
    .collect();
  let tris2: Vec<[usize; 3]> = m
    .faces
    .iter()
    .map(|(_, f)| [k2i2[&f.vertices[0]], k2i2[&f.vertices[1]], k2i2[&f.vertices[2]]])
    .collect();
  let n2 = pos2.len();

  // pre-slit identity for each slit vert (clones inherit their source)
  let clone2orig: FxHashMap<VertexKey, VertexKey> =
    banks.iter().chain(tip_clones.iter()).map(|&(c, cl)| (cl, c)).collect();
  let weld: Vec<usize> = keys2
    .iter()
    .map(|k| k2i[clone2orig.get(k).unwrap_or(k)])
    .collect();
  let along: Vec<f64> = (0..n2).map(|i| along0[weld[i]]).collect();

  // V (around harmonic): banks pinned 0/1 on the slit mesh
  let l2 = cotan_triplets(&pos2, &tris2);
  let pins: Vec<(usize, f64)> = banks
    .iter()
    .flat_map(|&(c, cl)| [(k2i2[&c], 0.), (k2i2[&cl], 1.)])
    .collect();
  let mut around = harmonic_interp(n2, &l2, &pins)?;

  // arc-length reparametrization of `along` (harmonic U is compressed near the pinned tips)
  let (arc_table, spine_len) = arc_reparam(&pos2, &along);
  let u2arc = |u: f64| arc_lookup(&arc_table, u);

  // De-twist: cancel rotational drift of `around` along the spine.  Wherever the seam wanders
  // azimuthally (corners, tip approaches), the pinned U=0 meridian drags the whole field with it.
  // Measure each arc-bin's phase offset against a rotation-minimizing frame transported along the
  // bin-centroid spine, then subtract the smoothed offset curve.  Bank pairs share along, so their
  // exact ±1 difference (the seamless wrap) is preserved.
  if opts.detwist {
    const KB: usize = 32;
    let tau = std::f64::consts::TAU;
    let (mut sum, mut cnt) = (vec![V3::zeros(); KB], vec![0usize; KB]);
    let mut bin_of = vec![0usize; n2];
    for i in 0..n2 {
      let b = ((u2arc(along[i]) * KB as f64) as usize).min(KB - 1);
      bin_of[i] = b;
      sum[b] += pos2[i];
      cnt[b] += 1;
    }
    let filled: Vec<usize> = (0..KB).filter(|&b| cnt[b] >= 4).collect();
    if filled.len() >= 3 {
      let cent = |b: usize| sum[b] / cnt[b] as f64;
      let nb_ = filled.len();
      let mut frames: Vec<(V3, V3)> = Vec::with_capacity(nb_);
      for (j, _) in filled.iter().enumerate() {
        let t = (cent(filled[(j + 1).min(nb_ - 1)]) - cent(filled[j.saturating_sub(1)])).normalize();
        let r = if j == 0 {
          let seed = if t.x.abs() < 0.9 { V3::new(1., 0., 0.) } else { V3::new(0., 1., 0.) };
          (seed - t * seed.dot(&t)).normalize()
        } else {
          let rp = frames[j - 1].1;
          let proj = rp - t * rp.dot(&t);
          if proj.norm() > 1e-9 {
            proj.normalize()
          } else {
            frames[j - 1].1
          }
        };
        frames.push((t, r));
      }
      let mut bin_slot = vec![usize::MAX; KB];
      for (j, &b) in filled.iter().enumerate() {
        bin_slot[b] = j;
      }
      // circular mean per bin of (2pi*around -/+ frame phase); handedness of `around` vs the frame
      // is unknown, so accumulate both and keep whichever is more concentrated
      let (mut sm, mut sp) = (vec![(0f64, 0f64); nb_], vec![(0f64, 0f64); nb_]);
      for i in 0..n2 {
        let j = bin_slot[bin_of[i]];
        if j == usize::MAX {
          continue;
        }
        let (t, r) = frames[j];
        let d = pos2[i] - cent(filled[j]);
        let phi = d.dot(&t.cross(&r)).atan2(d.dot(&r));
        for (acc, th) in [(&mut sm[j], tau * around[i] - phi), (&mut sp[j], tau * around[i] + phi)] {
          acc.0 += th.sin();
          acc.1 += th.cos();
        }
      }
      let conc = |v: &[(f64, f64)]| v.iter().map(|&(s, c)| s.hypot(c)).sum::<f64>();
      let raw = if conc(&sm) >= conc(&sp) { &sm } else { &sp };
      let mut offs: Vec<f64> = raw.iter().map(|&(s, c)| s.atan2(c)).collect();
      for j in 1..nb_ {
        let d = offs[j] - offs[j - 1];
        offs[j] -= (d / tau).round() * tau;
      }
      let base = offs[0];
      let xs: Vec<f64> = filled.iter().map(|&b| (b as f64 + 0.5) / KB as f64).collect();
      let off_at = |a: f64| -> f64 {
        if a <= xs[0] {
          return offs[0];
        }
        if a >= xs[nb_ - 1] {
          return offs[nb_ - 1];
        }
        let j = xs.partition_point(|&x| x < a).max(1);
        let f = ((a - xs[j - 1]) / (xs[j] - xs[j - 1])).clamp(0., 1.);
        offs[j - 1] * (1. - f) + offs[j] * f
      };
      for i in 0..n2 {
        around[i] -= (off_at(u2arc(along[i])) - base) / tau;
      }
    }
  }

  #[cfg(test)]
  if let Ok(path) = std::env::var("GEO_TUBE_DEBUG_DUMP") {
    use std::io::Write;
    let mut f = std::fs::File::create(&path).unwrap();
    writeln!(f, "{} {} {}", n2, tris2.len(), cutpath.len()).unwrap();
    for i in 0..n2 {
      let p = pos2[i];
      writeln!(f, "{} {} {} {} {}", p.x, p.y, p.z, along[i], around[i]).unwrap();
    }
    for t in &tris2 {
      writeln!(f, "{} {} {}", t[0], t[1], t[2]).unwrap();
    }
    for &k in &cutpath {
      let p = m.vertices[k].position;
      writeln!(f, "{} {} {}", p.x, p.y, p.z).unwrap();
    }
  }
  // mid cross-section perimeter (length of the along=0.5 level-set polyline) sets texel density
  let xsec_perim: f64 = {
    let mut acc = 0f64;
    for t in &tris2 {
      let mut hits: smallvec::SmallVec<[V3; 2]> = smallvec::SmallVec::new();
      for k in 0..3 {
        let (x, y) = (t[k], t[(k + 1) % 3]);
        let (ax, ay) = (along[x] - 0.5, along[y] - 0.5);
        if ax * ay < 0. {
          let f = ax / (ax - ay);
          hits.push(pos2[x] + (pos2[y] - pos2[x]) * f);
        }
      }
      if hits.len() == 2 {
        acc += (hits[1] - hits[0]).norm();
      }
    }
    acc.max(1e-9)
  };

  let fnorm = |t: &[usize; 3]| -> V3 {
    let n = (pos2[t[1]] - pos2[t[0]]).cross(&(pos2[t[2]] - pos2[t[0]]));
    let l = n.norm();
    if l > 1e-12 {
      n / l
    } else {
      V3::zeros()
    }
  };

  // cap detection: crease-bounded patch at each tip, transverse to the local end tangent, spanning
  // ~zero spine arc length
  let mut face_end: Vec<i8> = vec![-1; tris2.len()];
  let mut cap_axes = [V3::zeros(); 2];
  if opts.caps {
    // adjacency in pre-slit identity so patch growth sees the original closed topology (our own
    // seam/tip cuts run through cap interiors and must not fragment them)
    let mut edge_adj: FxHashMap<(usize, usize), smallvec::SmallVec<[usize; 2]>> = FxHashMap::default();
    for (fi, t) in tris2.iter().enumerate() {
      for k in 0..3 {
        let (x, y) = (weld[t[k]].min(weld[t[(k + 1) % 3]]), weld[t[k]].max(weld[t[(k + 1) % 3]]));
        edge_adj.entry((x, y)).or_default().push(fi);
      }
    }
    let end_tangents: [V3; 2] = {
      let band = |lo: f64, hi: f64| -> V3 {
        let (mut s, mut n) = (V3::zeros(), 0usize);
        for i in 0..n2 {
          let a = u2arc(along[i]);
          if a >= lo && a <= hi {
            s += pos2[i];
            n += 1;
          }
        }
        s / n.max(1) as f64
      };
      [
        (band(0., 0.07) - band(0.07, 0.3)).normalize(),
        (band(0.93, 1.) - band(0.7, 0.93)).normalize(),
      ]
    };
    let sharp_cos = opts.cap_angle_rad.unwrap_or(sharp_threshold_rad).cos() as f64;
    let tip_weld = [k2i[&ak], k2i[&bk]];
    for end in 0..2 {
      let tip_faces: Vec<usize> = (0..tris2.len())
        .filter(|&fi| tris2[fi].iter().any(|&v| weld[v] == tip_weld[end]))
        .collect();
      let mut best: Option<(f64, Vec<usize>, V3)> = None;
      let mut seeded: Vec<usize> = Vec::new();
      for &seed in &tip_faces {
        if seeded.contains(&seed) {
          continue;
        }
        let mut patch = vec![seed];
        let mut queue = vec![seed];
        while let Some(fi) = queue.pop() {
          let t = tris2[fi];
          for k in 0..3 {
            let (x, y) = (weld[t[k]].min(weld[t[(k + 1) % 3]]), weld[t[k]].max(weld[t[(k + 1) % 3]]));
            for &nb in &edge_adj[&(x, y)] {
              if nb != fi && !patch.contains(&nb) && fnorm(&tris2[nb]).dot(&fnorm(&tris2[fi])) > sharp_cos {
                patch.push(nb);
                queue.push(nb);
              }
            }
          }
        }
        seeded.extend(patch.iter().filter(|fi| tip_faces.contains(fi)));
        let n_seed = fnorm(&tris2[seed]);
        let mut axis = V3::zeros();
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        for &fi in &patch {
          let n = fnorm(&tris2[fi]);
          axis += if n.dot(&n_seed) < 0. { -n } else { n };
          for &vtx in &tris2[fi] {
            lo = lo.min(u2arc(along[vtx]));
            hi = hi.max(u2arc(along[vtx]));
          }
        }
        let axis = axis.normalize();
        if axis.dot(&end_tangents[end]).abs() < opts.cap_alignment {
          continue;
        }
        let span = hi - lo;
        if best.as_ref().is_none_or(|(s, ..)| span < *s) {
          best = Some((span, patch, axis));
        }
      }
      if let Some((span, patch, axis)) = best {
        if span < opts.cap_max_span {
          for fi in patch {
            face_end[fi] = end as i8;
          }
          cap_axes[end] = axis;
        }
      }
    }
  }

  // output arrays: body UVs are (around, along-arc); caps get overwritten below
  let mut verts: Vec<f32> = Vec::with_capacity(n2 * 3);
  for p in &pos2 {
    verts.extend_from_slice(&[p.x as f32, p.y as f32, p.z as f32]);
  }
  let mut indices: Vec<u32> = tris2.iter().flat_map(|t| t.iter().map(|&i| i as u32)).collect();
  let v_scale = if opts.normalize_v { 1. } else { spine_len / xsec_perim };
  let mut us: Vec<f32> = (0..n2).map(|i| around[i] as f32).collect();
  let mut vs: Vec<f32> = (0..n2).map(|i| (u2arc(along[i]) * v_scale) as f32).collect();

  // per-vertex tangent along +U (the around direction) from per-face gradients of `around`; the
  // slit-mesh duplicates make the gradient seam-consistent
  let mut tangents: Vec<[f32; 4]> = {
    let mut acc = vec![V3::zeros(); n2];
    for t in &tris2 {
      let (p0, p1, p2) = (pos2[t[0]], pos2[t[1]], pos2[t[2]]);
      let (e1, e2) = (p1 - p0, p2 - p0);
      let nrm = e1.cross(&e2);
      let d = nrm.dot(&nrm).max(1e-20);
      let g = (around[t[1]] - around[t[0]]) * e2.cross(&nrm) / d
        + (around[t[2]] - around[t[0]]) * nrm.cross(&e1) / d;
      for &vtx in t {
        acc[vtx] += g;
      }
    }
    acc
      .iter()
      .map(|g| {
        let l = g.norm();
        let t = if l > 1e-12 { g / l } else { V3::new(1., 0., 0.) };
        [t.x as f32, t.y as f32, t.z as f32, 1.]
      })
      .collect()
  };

  // caps: planar islands at body texel density, centered at UV origin (matches `cylindrical`);
  // rim verts shared with the body are cloned so body UVs stay untouched
  let mut used_by_body = vec![false; n2];
  for (fi, t) in tris2.iter().enumerate() {
    if face_end[fi] < 0 {
      for &vtx in t {
        used_by_body[vtx] = true;
      }
    }
  }
  for end in 0..2 {
    let cap_tris: Vec<usize> = (0..tris2.len()).filter(|&fi| face_end[fi] == end as i8).collect();
    if cap_tris.is_empty() {
      continue;
    }
    let mut cverts: Vec<usize> = Vec::new();
    let mut edge_count: FxHashMap<(usize, usize), usize> = FxHashMap::default();
    for &fi in &cap_tris {
      let t = tris2[fi];
      for k in 0..3 {
        if !cverts.contains(&t[k]) {
          cverts.push(t[k]);
        }
        let (x, y) = (weld[t[k]].min(weld[t[(k + 1) % 3]]), weld[t[k]].max(weld[t[(k + 1) % 3]]));
        *edge_count.entry((x, y)).or_insert(0) += 1;
      }
    }
    let perim: f64 = edge_count
      .iter()
      .filter(|(_, &c)| c == 1)
      .map(|(&(x, y), _)| (pos[x] - pos[y]).norm())
      .sum::<f64>()
      .max(1e-9);
    let center = cverts.iter().map(|&i| pos2[i]).sum::<V3>() / cverts.len() as f64;
    let axis32 = Vec3::new(cap_axes[end].x as f32, cap_axes[end].y as f32, cap_axes[end].z as f32);
    let (b1, b2) = orthonormal_basis(axis32);
    let (b1d, b2d) = (
      V3::new(b1.x as f64, b1.y as f64, b1.z as f64),
      V3::new(b2.x as f64, b2.y as f64, b2.z as f64),
    );
    let mut cap_dup: FxHashMap<u32, u32> = FxHashMap::default();
    for &fi in &cap_tris {
      let base = fi * 3;
      for slot in 0..3 {
        let o = indices[base + slot] as usize;
        let d = pos2[o] - center;
        let uv = [(d.dot(&b1d) / perim) as f32, (d.dot(&b2d) / perim) as f32];
        let tan = [b1.x, b1.y, b1.z, 1.];
        if used_by_body[o] {
          indices[base + slot] = *cap_dup.entry(o as u32).or_insert_with(|| {
            let new_i = (verts.len() / 3) as u32;
            verts.extend_from_slice(&[pos2[o].x as f32, pos2[o].y as f32, pos2[o].z as f32]);
            us.push(uv[0]);
            vs.push(uv[1]);
            tangents.push(tan);
            new_i
          });
        } else {
          us[o] = uv[0];
          vs[o] = uv[1];
          tangents[o] = tan;
        }
      }
    }
  }

  let mut out = LinkedMesh::from_raw_indexed(&verts, &indices, None, None);
  let mut uv_ch = new_uv_channel();
  let mut tan_ch = new_tangent_channel();
  for i in 0..(verts.len() / 3) {
    let key = vkey(i as u32 + 1, 1);
    uv_ch.set(key, [us[i] * scale, vs[i] * scale, 0., 0.]);
    tan_ch.set(key, tangents[i]);
  }
  out.vertex_channels.insert("uv".to_owned(), uv_ch);
  out.vertex_channels.insert("tangent".to_owned(), tan_ch);
  out.mark_edge_sharpness(sharp_threshold_rad);
  out.separate_vertices_and_compute_normals();
  out.flags |= mesh_flags::NO_WELD;

  Ok(MeshHandle {
    mesh: Rc::new(out),
    transform: mesh.transform,
    manifold_handle: Rc::new(ManifoldHandle::new(0)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: mesh.material.clone(),
  })
}

#[cfg(test)]
mod tests {
  use mesh::linked_mesh::{mesh_flags, ChannelStore};

  fn rendered_mesh(src: &str) -> Rc<mesh::LinkedMesh<()>> {
    let ctx = crate::parse_and_eval_program(src).unwrap();
    Rc::clone(&ctx.rendered_meshes.into_inner()[0].mesh.mesh)
  }

  use std::rc::Rc;

  fn uvs(mesh: &mesh::LinkedMesh<()>) -> Vec<[f32; 2]> {
    let ChannelStore::Vec2(uv) = &mesh.vertex_channels["uv"].store else {
      panic!("uv channel should be Vec2");
    };
    uv.values().copied().collect()
  }

  /// No face may bridge the U = 0/1 seam; that's what makes a tiling texture seamless.
  fn max_face_u_span(mesh: &mesh::LinkedMesh<()>) -> f32 {
    let uv = &mesh.vertex_channels["uv"];
    let mut worst = 0f32;
    for (_, face) in mesh.iter_faces() {
      let (mut lo, mut hi) = (f32::MAX, f32::MIN);
      for &v in &face.vertices {
        let u = uv.get(v).unwrap()[0];
        lo = lo.min(u);
        hi = hi.max(u);
      }
      worst = worst.max(hi - lo);
    }
    worst
  }

  #[test]
  fn tube_on_straight_cylinder() {
    let mesh = rendered_mesh("cylinder(1, 4, 24) | compute_uvs(type='tube') | render");
    assert!(mesh.has_flag(mesh_flags::NO_WELD));
    assert!(mesh.vertex_channels.contains_key("tangent"));
    assert!(max_face_u_span(&mesh) < 0.5, "face bridges U seam: {}", max_face_u_span(&mesh));
    for uv in uvs(&mesh) {
      assert!(uv[0].is_finite() && uv[1].is_finite(), "non-finite UV {uv:?}");
    }
    // height 4 / circumference 2pi -> isotropic V spans ~0.64; caps' islands stay near origin
    let max_v = uvs(&mesh).iter().map(|uv| uv[1]).fold(f32::MIN, f32::max);
    assert!(max_v > 0.3 && max_v < 2., "isotropic V should be arc/perimeter-scaled, got {max_v}");
  }

  #[test]
  fn tube_on_bent_pipe() {
    let src = "extrude_pipe(radius=0.4, resolution=8, path=0..16 -> |i| { t = i / 15\n v3(sin(t*pi)*2.5, t*5, 0) }) | compute_uvs(type='tube') | render";
    let mesh = rendered_mesh(src);
    assert!(max_face_u_span(&mesh) < 0.5, "face bridges U seam: {}", max_face_u_span(&mesh));
    let all = uvs(&mesh);
    for uv in &all {
      assert!(uv[0].is_finite() && uv[1].is_finite(), "non-finite UV {uv:?}");
    }
    // long thin tube: isotropic V must span multiple wraps (spine_len >> cross-section perimeter)
    let max_v = all.iter().map(|uv| uv[1]).fold(f32::MIN, f32::max);
    assert!(max_v > 1.5, "expected multiple V wraps on a long tube, got max_v {max_v}");
  }

  #[test]
  fn tube_normalize_v() {
    let src = "extrude_pipe(radius=0.4, resolution=8, path=0..16 -> |i| { t = i / 15\n v3(sin(t*pi)*2.5, t*5, 0) }) | compute_uvs(type='tube', options={ normalize_v: true }) | render";
    let mesh = rendered_mesh(src);
    let max_v = uvs(&mesh).iter().map(|uv| uv[1]).fold(f32::MIN, f32::max);
    assert!(max_v > 0.9 && max_v < 1.6, "normalize_v should make V span ~0..1, got max_v {max_v}");
  }

  #[test]
  fn tube_debug_dump_c_ring() {
    if std::env::var("GEO_TUBE_DEBUG_DUMP").is_err() {
      return;
    }
    // C-shaped arc tube (mesh CSG is wasm-only, so build the ring class via extrude_pipe)
    let src = "extrude_pipe(radius=0.7, resolution=8, path=0..49 -> |i| {\n\
                 t = 0.05 + 0.9 * (i / 48)\n\
                 a = t * pi * 2\n\
                 v3(cos(a) * 4, 0, sin(a) * 4)\n\
               }) | compute_uvs(type='tube') | render";
    crate::parse_and_eval_program(src).unwrap();
  }

  #[test]
  fn tube_rejects_genus_1() {
    // hand-built torus: mesh CSG is wasm-only in tests (native eval_mesh_boolean returns an
    // empty mesh), so construct genus-1 directly
    let (nu, nv) = (12usize, 8usize);
    let mut verts = Vec::new();
    for i in 0..nu {
      let a = i as f32 / nu as f32 * std::f32::consts::TAU;
      for j in 0..nv {
        let b = j as f32 / nv as f32 * std::f32::consts::TAU;
        let r = 3. + b.cos();
        verts.push(mesh::linked_mesh::Vec3::new(a.cos() * r, b.sin(), a.sin() * r));
      }
    }
    let mut idx: Vec<u32> = Vec::new();
    for i in 0..nu {
      for j in 0..nv {
        let (i1, j1) = ((i + 1) % nu, (j + 1) % nv);
        let [a, b, c, d] = [i * nv + j, i1 * nv + j, i1 * nv + j1, i * nv + j1].map(|x| x as u32);
        idx.extend_from_slice(&[a, b, c, a, c, d]);
      }
    }
    let lm = mesh::LinkedMesh::from_indexed_vertices(&verts, &idx, None, None);
    let handle = crate::MeshHandle::new(Rc::new(lm));
    let err = super::tube_uvs(&handle, 1., 0.8, None).unwrap_err();
    assert!(format!("{err}").contains("Euler"), "got: {err}");
  }

  #[test]
  fn tube_unknown_option_errors() {
    let err = crate::parse_and_eval_program(
      "cylinder(1, 4, 24) | compute_uvs(type='tube', options={ bogus: 1 }) | render",
    )
    .unwrap_err();
    assert!(format!("{err}").contains("Unknown option"), "got: {err}");
  }
}
