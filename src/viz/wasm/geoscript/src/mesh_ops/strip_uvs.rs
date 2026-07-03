//! Topological trim-strip unwrap.  The mesh is split at sharp edges into smooth patches; a patch
//! whose face-adjacency dual graph is a path or cycle (a triangulated quad/tri strip) is mapped
//! STRAIGHT in UV space: U = cumulative arc length along each of the strip's two boundary rails,
//! V = constant per rail.  Deliberately trades angle preservation for straight texture flow along
//! curved strips (the trim-sheet workflow).  Closed strips (rings) are cut at their most
//! transverse hinge and get an integer number of U repeats so tiling textures stay seamless.
//! Non-strip patches fall back to per-patch planar islands.

use std::{cell::RefCell, rc::Rc};

use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{mesh_flags, EdgeKey, FaceKey, Vec3, VertexKey},
  LinkedMesh,
};

use super::compute_uvs::{new_tangent_channel, new_uv_channel, orthonormal_basis};
use crate::{ErrorStack, ManifoldHandle, MeshHandle, Value};

#[derive(Clone, Copy, PartialEq)]
enum Layout {
  /// Islands stacked in V, each starting at an integer V so tiling textures row-align.
  Stack,
  /// All islands share the V band starting at 0 (sample the same texture rows).
  Overlap,
  /// Each island's V spans 0..1 (full texture height); U keeps square texels relative to that.
  Fill,
}

#[derive(Clone, Copy, PartialEq)]
enum UMode {
  /// Both rails share U (mean arc length): quads map to true rectangles.  A ring band's inner
  /// rail stretches rather than shearing the texture around the ring.
  Uniform,
  /// Each rail keeps its own arc length: per-rail texel density is exact, but rails of different
  /// lengths shear the map.
  Rail,
}

struct StripOptions {
  strip_angle_rad: Option<f32>,
  layout: Layout,
  u_mode: UMode,
  planar_fallback: bool,
}

fn parse_strip_options(options: Option<&FxHashMap<String, Value>>) -> Result<StripOptions, ErrorStack> {
  let mut out = StripOptions {
    strip_angle_rad: None,
    layout: Layout::Stack,
    u_mode: UMode::Uniform,
    planar_fallback: true,
  };
  let Some(map) = options else { return Ok(out) };
  for (key, val) in map {
    match key.as_str() {
      "strip_angle" => {
        let deg = val.as_float().ok_or_else(|| {
          ErrorStack::new("`compute_uvs` option `strip_angle` must be a number (degrees)")
        })?;
        out.strip_angle_rad = Some(deg.to_radians());
      }
      "layout" => {
        out.layout = match val.as_str() {
          Some("stack") => Layout::Stack,
          Some("overlap") => Layout::Overlap,
          Some("fill") => Layout::Fill,
          _ => {
            return Err(ErrorStack::new(
              "`compute_uvs` option `layout` must be 'stack', 'overlap', or 'fill'",
            ))
          }
        };
      }
      "u_mode" => {
        out.u_mode = match val.as_str() {
          Some("uniform") => UMode::Uniform,
          Some("rail") => UMode::Rail,
          _ => {
            return Err(ErrorStack::new(
              "`compute_uvs` option `u_mode` must be 'uniform' or 'rail'",
            ))
          }
        };
      }
      "fallback" => {
        out.planar_fallback = match val.as_str() {
          Some("planar") => true,
          Some("error") => false,
          _ => {
            return Err(ErrorStack::new(
              "`compute_uvs` option `fallback` must be 'planar' or 'error'",
            ))
          }
        };
      }
      _ => {
        return Err(ErrorStack::new(format!(
          "Unknown option {key:?} for `compute_uvs(type='strip')`; supported options: \
           `strip_angle`, `layout`, `u_mode`, `fallback`"
        )))
      }
    }
  }
  Ok(out)
}

fn vp(m: &LinkedMesh<()>, v: VertexKey) -> Vec3 {
  m.vertices[v].position
}

fn fcentroid(m: &LinkedMesh<()>, f: FaceKey) -> Vec3 {
  let [a, b, c] = m.faces[f].vertices;
  (vp(m, a) + vp(m, b) + vp(m, c)) / 3.
}

fn farea(m: &LinkedMesh<()>, f: FaceKey) -> f32 {
  let [a, b, c] = m.faces[f].vertices;
  (vp(m, b) - vp(m, a)).cross(&(vp(m, c) - vp(m, a))).norm() * 0.5
}

fn edge_other(m: &LinkedMesh<()>, e: EdgeKey, v: VertexKey) -> VertexKey {
  let vs = m.edges[e].vertices;
  if vs[0] == v {
    vs[1]
  } else {
    vs[0]
  }
}

/// Dual-graph neighbors: faces across this face's interior (2-face) edges.
fn face_nbrs(m: &LinkedMesh<()>, f: FaceKey) -> Vec<FaceKey> {
  m.faces[f]
    .edges
    .iter()
    .filter_map(|&e| match m.edges[e].faces[..] {
      [a, b] => Some(if a == f { b } else { a }),
      _ => None,
    })
    .collect()
}

fn boundary_edges(m: &LinkedMesh<()>, f: FaceKey) -> Vec<EdgeKey> {
  m.faces[f]
    .edges
    .iter()
    .copied()
    .filter(|&e| m.edges[e].faces.len() == 1)
    .collect()
}

fn shared_edge(m: &LinkedMesh<()>, f: FaceKey, g: FaceKey) -> Option<EdgeKey> {
  m.faces[f].edges.iter().copied().find(|&e| m.edges[e].faces.contains(&g))
}

fn faces_of_vertex(m: &LinkedMesh<()>, v: VertexKey) -> FxHashSet<FaceKey> {
  let mut out = FxHashSet::default();
  for &e in &m.vertices[v].edges {
    out.extend(m.edges[e].faces.iter().copied());
  }
  out
}

fn walk_rail(
  m: &LinkedMesh<()>,
  vb: &FxHashMap<VertexKey, Vec<EdgeKey>>,
  start_v: VertexKey,
  start_rung: EdgeKey,
  stop: [VertexKey; 2],
) -> Option<Vec<VertexKey>> {
  let mut rail = vec![start_v];
  let (mut cur, mut pe) = (start_v, start_rung);
  for _ in 0..=vb.len() {
    if stop.contains(&cur) {
      return Some(rail);
    }
    let es = vb.get(&cur)?;
    if es.len() != 2 {
      return None;
    }
    let &e = es.iter().find(|&&e| e != pe)?;
    cur = edge_other(m, e, cur);
    pe = e;
    rail.push(cur);
  }
  None
}

struct Island {
  uvs: Vec<(VertexKey, [f32; 2])>,
  v_span: f32,
  u_extent: f32,
}

/// Flip V if the UV winding is mirrored relative to the 3D face winding.
fn mirror_fix(m: &LinkedMesh<()>, faces: &[FaceKey], uvs: &mut [(VertexKey, [f32; 2])], v_span: f32) {
  let map: FxHashMap<VertexKey, [f32; 2]> = uvs.iter().copied().collect();
  let signed: f32 = faces
    .iter()
    .map(|&f| {
      let [a, b, c] = m.faces[f].vertices;
      let (u0, u1, u2) = (map[&a], map[&b], map[&c]);
      (u1[0] - u0[0]) * (u2[1] - u0[1]) - (u1[1] - u0[1]) * (u2[0] - u0[0])
    })
    .sum();
  if signed < 0. {
    for (_, uv) in uvs {
      uv[1] = v_span - uv[1];
    }
  }
}

fn try_strip(m: &mut LinkedMesh<()>, faces: &[FaceKey], scale: f32, opts: &StripOptions) -> Option<Island> {
  let n = faces.len();
  if n < 2 {
    return None;
  }
  for &f in faces {
    if face_nbrs(m, f).len() > 2 {
      return None;
    }
  }
  let ends: Vec<FaceKey> = faces.iter().copied().filter(|&f| face_nbrs(m, f).len() <= 1).collect();
  let (start, closed) = match ends.len() {
    2 => (ends[0], false),
    0 => (faces[0], true),
    _ => return None,
  };
  if closed && n < 4 {
    return None;
  }

  let mut ordered = vec![start];
  let mut prev = None;
  loop {
    let cur = *ordered.last().unwrap();
    match face_nbrs(m, cur).into_iter().find(|&f| Some(f) != prev) {
      Some(nf) if !(closed && nf == start) => {
        prev = Some(cur);
        ordered.push(nf);
      }
      _ => break,
    }
  }
  if ordered.len() != n {
    return None;
  }

  let mut cut_rungs = None;
  if closed {
    // cut the ring at its most transverse hinge so the seam runs straight across the strip
    let best = (0..n).min_by(|&i, &j| {
      let score = |i: usize| -> f32 {
        let (f, g) = (ordered[i], ordered[(i + 1) % n]);
        let Some(e) = shared_edge(m, f, g) else { return f32::MAX };
        let [a, b] = m.edges[e].vertices;
        match (
          (vp(m, b) - vp(m, a)).try_normalize(1e-12),
          (fcentroid(m, g) - fcentroid(m, f)).try_normalize(1e-12),
        ) {
          (Some(ed), Some(d)) => ed.dot(&d).abs(),
          _ => f32::MAX,
        }
      };
      score(i).total_cmp(&score(j))
    })?;
    ordered.rotate_left(best + 1);
    let e_cut = shared_edge(m, ordered[n - 1], ordered[0])?;
    let widx: FxHashMap<FaceKey, usize> = ordered.iter().enumerate().map(|(i, &f)| (f, i)).collect();
    let [va, vbk] = m.edges[e_cut].vertices;
    let mut clones = Vec::with_capacity(2);
    for v in [va, vbk] {
      let mut idxs: Vec<usize> = faces_of_vertex(m, v).iter().map(|f| widx[f]).collect();
      idxs.sort_unstable();
      // the fan crosses the seam: a contiguous run ..n-1 plus a prefix run from 0; move the prefix
      let mut k = 0;
      while k + 1 < idxs.len() && idxs[k + 1] == idxs[k] + 1 {
        k += 1;
      }
      if k + 1 == idxs.len() {
        return None;
      }
      let cut_faces: Vec<FaceKey> = idxs[..=k].iter().map(|&i| ordered[i]).collect();
      clones.push(m.split_off_faces(v, &cut_faces));
    }
    cut_rungs = Some((m.get_edge_key([clones[0], clones[1]])?, e_cut));
  }

  let mut vb: FxHashMap<VertexKey, Vec<EdgeKey>> = FxHashMap::default();
  for &f in &ordered {
    for e in boundary_edges(m, f) {
      for v in m.edges[e].vertices {
        vb.entry(v).or_default().push(e);
      }
    }
  }
  let n_verts: usize = {
    let mut set = FxHashSet::default();
    for &f in &ordered {
      set.extend(m.faces[f].vertices);
    }
    set.len()
  };
  let rails_for = |start_rung: EdgeKey, end_rung: EdgeKey| -> Option<(Vec<VertexKey>, Vec<VertexKey>)> {
    let stop = m.edges[end_rung].vertices;
    let [sa, sb] = m.edges[start_rung].vertices;
    let rail_a = walk_rail(m, &vb, sa, start_rung, stop)?;
    let rail_b = walk_rail(m, &vb, sb, start_rung, stop)?;
    (rail_a.len() >= 2
      && rail_b.len() >= 2
      && rail_a.last() != rail_b.last()
      && rail_a.len() + rail_b.len() == n_verts)
      .then_some((rail_a, rail_b))
  };

  let area: f32 = ordered.iter().map(|&f| farea(m, f)).sum();
  let patch_edges: Vec<EdgeKey> = {
    let mut set = FxHashSet::default();
    for &f in &ordered {
      set.extend(m.faces[f].edges);
    }
    set.into_iter().collect()
  };

  struct Reading {
    rail_a: Vec<VertexKey>,
    rail_b: Vec<VertexKey>,
    arcs_a: Vec<f32>,
    arcs_b: Vec<f32>,
    w: f32,
    err: f32,
  }
  // Score a (start rung, end rung) reading by metric distortion: mean |uv edge length - 3d edge
  // length|.  A mis-paired reading (walks leaking across an end ring) can pass every structural
  // check — rails 2-coverage and all — but stretches every rung, so the metric separates true and
  // sheared readings by an order of magnitude.  Direction heuristics were tried and are too
  // fragile: the first dual step points diagonally across wide quads, and near-tie scores flip
  // between native and wasm libm ulps.
  let evaluate = |rail_a: Vec<VertexKey>, rail_b: Vec<VertexKey>| -> Option<Reading> {
    let arc = |rail: &[VertexKey]| -> Vec<f32> {
      let mut out = Vec::with_capacity(rail.len());
      let mut acc = 0f32;
      out.push(0.);
      for w in rail.windows(2) {
        acc += (vp(m, w[1]) - vp(m, w[0])).norm();
        out.push(acc);
      }
      out
    };
    let (mut arcs_a, mut arcs_b) = (arc(&rail_a), arc(&rail_b));
    let (la, lb) = (*arcs_a.last().unwrap(), *arcs_b.last().unwrap());
    if la < 1e-6 || lb < 1e-6 {
      return None;
    }
    // uniform U: both rails share mean arc length so quads map to true rectangles (a ring band's
    // shorter inner rail stretches instead of shearing the map).  Falls back to per-rail arcs
    // when the rails disagree on vertex count (non-ladder strip).
    if opts.u_mode == UMode::Uniform && arcs_a.len() == arcs_b.len() {
      for i in 0..arcs_a.len() {
        let mean = (arcs_a[i] + arcs_b[i]) * 0.5;
        arcs_a[i] = mean;
        arcs_b[i] = mean;
      }
    }
    let w = (area / ((la + lb) * 0.5)).max(1e-6);
    let mut uv: FxHashMap<VertexKey, [f32; 2]> = FxHashMap::default();
    for (rail, arcs, v) in [(&rail_a, &arcs_a, 0f32), (&rail_b, &arcs_b, w)] {
      for (i, &vk) in rail.iter().enumerate() {
        uv.insert(vk, [arcs[i], v]);
      }
    }
    let (mut num, mut den) = (0f32, 0f32);
    for &e in &patch_edges {
      let [a, b] = m.edges[e].vertices;
      let l3 = (vp(m, b) - vp(m, a)).norm();
      let (ua, ub) = (uv[&a], uv[&b]);
      num += ((ua[0] - ub[0]).hypot(ua[1] - ub[1]) - l3).abs();
      den += l3;
    }
    Some(Reading { rail_a, rail_b, arcs_a, arcs_b, w, err: num / den.max(1e-12) })
  };

  let reading = match cut_rungs {
    Some((sr, er)) => {
      let (a, b) = rails_for(sr, er)?;
      evaluate(a, b)?
    }
    None => {
      let mut best: Option<Reading> = None;
      for sr in boundary_edges(m, ordered[0]) {
        for er in boundary_edges(m, ordered[n - 1]) {
          let Some(r) = rails_for(sr, er).and_then(|(a, b)| evaluate(a, b)) else {
            continue;
          };
          if best.as_ref().map_or(true, |b| r.err < b.err) {
            best = Some(r);
          }
        }
      }
      best?
    }
  };

  let (u_scale, v_span) = match opts.layout {
    Layout::Fill => (scale / reading.w, 1f32),
    _ => (scale, reading.w * scale),
  };

  let mut uvs = Vec::with_capacity(n_verts);
  let mut u_extent = 0f32;
  for (rail, arcs, v) in [
    (&reading.rail_a, &reading.arcs_a, 0f32),
    (&reading.rail_b, &reading.arcs_b, v_span),
  ] {
    let total = arcs.last().unwrap() * u_scale;
    // integer repeat count keeps a tiling texture seamless across the ring's cut
    let fac = if closed && total > 1e-6 { total.round().max(1.) / total } else { 1. };
    u_extent = u_extent.max(total * fac);
    for (i, &vk) in rail.iter().enumerate() {
      uvs.push((vk, [arcs[i] * u_scale * fac, v]));
    }
  }
  mirror_fix(m, &ordered, &mut uvs, v_span);
  Some(Island { uvs, v_span, u_extent })
}

fn planar_island(m: &LinkedMesh<()>, faces: &[FaceKey], scale: f32, layout: Layout) -> Island {
  let mut normal = Vec3::zeros();
  for &f in faces {
    let [a, b, c] = m.faces[f].vertices;
    normal += (vp(m, b) - vp(m, a)).cross(&(vp(m, c) - vp(m, a)));
  }
  let normal = normal.try_normalize(1e-12).unwrap_or(Vec3::y());
  let (u_axis, v_axis) = orthonormal_basis(normal);

  let mut verts = FxHashSet::default();
  for &f in faces {
    verts.extend(m.faces[f].vertices);
  }
  let mut uvs: Vec<(VertexKey, [f32; 2])> = verts
    .into_iter()
    .map(|v| {
      let p = vp(m, v);
      (v, [p.dot(&u_axis), p.dot(&v_axis)])
    })
    .collect();
  let (mut min_u, mut min_v, mut max_u, mut max_v) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
  for (_, uv) in &uvs {
    min_u = min_u.min(uv[0]);
    min_v = min_v.min(uv[1]);
    max_u = max_u.max(uv[0]);
    max_v = max_v.max(uv[1]);
  }
  let factor = match layout {
    Layout::Fill => 1. / (max_v - min_v).max(1e-6),
    _ => scale,
  };
  for (_, uv) in &mut uvs {
    uv[0] = (uv[0] - min_u) * factor;
    uv[1] = (uv[1] - min_v) * factor;
  }
  let v_span = (max_v - min_v) * factor;
  mirror_fix(m, faces, &mut uvs, v_span);
  Island {
    uvs,
    v_span,
    u_extent: (max_u - min_u) * factor,
  }
}

pub(crate) fn strip_uvs(
  mesh: &MeshHandle,
  scale: f32,
  sharp_threshold_rad: f32,
  options: Option<&FxHashMap<String, Value>>,
) -> Result<MeshHandle, ErrorStack> {
  let opts = parse_strip_options(options)?;
  let mut m = (*mesh.mesh).clone();
  if m.faces.is_empty() {
    return Err(ErrorStack::new("`compute_uvs(type='strip')`: mesh has no faces"));
  }

  // Sharp-split first: afterwards faces across sharp edges share no vertices, so smooth patches
  // are exactly the components of the interior-edge adjacency graph and per-patch UVs can't
  // collide at former corner vertices.  Shading normals come along for free.
  m.mark_edge_sharpness(opts.strip_angle_rad.unwrap_or(sharp_threshold_rad));
  m.separate_vertices_and_compute_normals();

  let fkeys: Vec<FaceKey> = m.faces.iter().map(|(k, _)| k).collect();
  let f2i: FxHashMap<FaceKey, usize> = fkeys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
  let mut uf: Vec<usize> = (0..fkeys.len()).collect();
  fn find(uf: &mut [usize], mut i: usize) -> usize {
    while uf[i] != i {
      uf[i] = uf[uf[i]];
      i = uf[i];
    }
    i
  }
  for (_, e) in m.edges.iter() {
    if let [a, b] = e.faces[..] {
      let (ra, rb) = (find(&mut uf, f2i[&a]), find(&mut uf, f2i[&b]));
      uf[ra] = rb;
    }
  }
  let mut groups: FxHashMap<usize, Vec<FaceKey>> = FxHashMap::default();
  for i in 0..fkeys.len() {
    groups.entry(find(&mut uf, i)).or_default().push(fkeys[i]);
  }
  let patches: Vec<Vec<FaceKey>> = groups.into_values().collect();

  let mut islands = Vec::with_capacity(patches.len());
  for faces in &patches {
    match try_strip(&mut m, faces, scale, &opts) {
      Some(island) => islands.push(island),
      None if opts.planar_fallback => islands.push(planar_island(&m, faces, scale, opts.layout)),
      None => {
        return Err(ErrorStack::new(format!(
          "`compute_uvs(type='strip')`: a {}-face patch is not a quad/tri strip (dual graph is \
           not a path or ring).  Use options={{ fallback: 'planar' }} to map such patches as \
           planar islands, or adjust `strip_angle` so patch boundaries land on strip seams",
          faces.len()
        )))
      }
    }
  }

  islands.sort_by(|a, b| b.u_extent.total_cmp(&a.u_extent));
  let mut final_uv: FxHashMap<VertexKey, [f32; 2]> = FxHashMap::default();
  let mut y = 0f32;
  for island in &islands {
    let off = match opts.layout {
      Layout::Stack => (y - 1e-4).ceil().max(0.),
      _ => 0.,
    };
    for &(v, uv) in &island.uvs {
      final_uv.insert(v, [uv[0], uv[1] + off]);
    }
    y = off + island.v_span;
  }

  // tangents from the UV gradient: ∂position/∂U accumulated per vertex
  let mut tan_acc: FxHashMap<VertexKey, Vec3> = FxHashMap::default();
  for (_, face) in m.iter_faces() {
    let [a, b, c] = face.vertices;
    let (uv0, uv1, uv2) = (final_uv[&a], final_uv[&b], final_uv[&c]);
    let (d1, d2) = (vp(&m, b) - vp(&m, a), vp(&m, c) - vp(&m, a));
    let (du1, dv1) = (uv1[0] - uv0[0], uv1[1] - uv0[1]);
    let (du2, dv2) = (uv2[0] - uv0[0], uv2[1] - uv0[1]);
    let det = du1 * dv2 - du2 * dv1;
    if det.abs() < 1e-10 {
      continue;
    }
    let t = (d1 * dv2 - d2 * dv1) / det;
    for v in [a, b, c] {
      *tan_acc.entry(v).or_insert_with(Vec3::zeros) += t;
    }
  }

  let mut uv_ch = new_uv_channel();
  let mut tan_ch = new_tangent_channel();
  for (&v, &uv) in &final_uv {
    uv_ch.set(v, [uv[0], uv[1], 0., 0.]);
    let t = tan_acc
      .get(&v)
      .and_then(|t| t.try_normalize(1e-6))
      .unwrap_or_else(Vec3::x);
    tan_ch.set(v, [t.x, t.y, t.z, 1.]);
  }
  m.vertex_channels.insert("uv".to_owned(), uv_ch);
  m.vertex_channels.insert("tangent".to_owned(), tan_ch);
  m.flags |= mesh_flags::NO_WELD;

  Ok(MeshHandle {
    mesh: Rc::new(m),
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
  use std::rc::Rc;

  fn rendered_mesh(src: &str) -> Rc<mesh::LinkedMesh<()>> {
    let ctx = crate::parse_and_eval_program(src).unwrap();
    Rc::clone(&ctx.rendered_meshes.into_inner()[0].mesh.mesh)
  }

  fn uvs(mesh: &mesh::LinkedMesh<()>) -> Vec<[f32; 2]> {
    let ChannelStore::Vec2(uv) = &mesh.vertex_channels["uv"].store else {
      panic!("uv channel should be Vec2");
    };
    uv.values().copied().collect()
  }

  const SQUARE_PIPE: &str = "extrude_pipe(5, 4, 0..20 -> |i| v3(i*3, sin(i * 0.1) * 10, 0), \
                             adaptive_path_sampling=false)";

  #[test]
  fn strip_on_square_pipe() {
    let mesh = rendered_mesh(&format!("{SQUARE_PIPE} | compute_uvs(type='strip') | render"));
    assert!(mesh.has_flag(mesh_flags::NO_WELD));
    assert!(mesh.vertex_channels.contains_key("tangent"));

    let all = uvs(&mesh);
    for uv in &all {
      assert!(uv[0].is_finite() && uv[1].is_finite(), "non-finite UV {uv:?}");
    }
    // strips mapped STRAIGHT: every vertex sits on a rail, so V takes at most 2 distinct values
    // per island (6 islands: 4 sides + 2 caps).  A curved/conformal unwrap would produce a
    // near-continuous V distribution.
    let mut distinct_v: Vec<f32> = Vec::new();
    for uv in &all {
      if !distinct_v.iter().any(|&v| (v - uv[1]).abs() < 1e-3) {
        distinct_v.push(uv[1]);
      }
    }
    assert!(distinct_v.len() <= 12, "expected <=2 V values per island, got {distinct_v:?}");
    // U runs the full length of the pipe in world units (path length > 55)
    let max_u = all.iter().map(|uv| uv[0]).fold(f32::MIN, f32::max);
    assert!(max_u > 50., "U should span the strip arc length, got max {max_u}");

    // near-isometry: a mis-paired rail reading passes the V check but shears U, stretching every
    // rung ~15%+; the true reading is within ~1%
    let uv_ch = &mesh.vertex_channels["uv"];
    let (mut num, mut den) = (0f32, 0f32);
    for (_, e) in mesh.edges.iter() {
      if e.faces.is_empty() {
        continue;
      }
      let [a, b] = e.vertices;
      let l3 = (mesh.vertices[a].position - mesh.vertices[b].position).norm();
      let (ua, ub) = (uv_ch.get(a).unwrap(), uv_ch.get(b).unwrap());
      num += ((ua[0] - ub[0]).hypot(ua[1] - ub[1]) - l3).abs();
      den += l3;
    }
    let distortion = num / den;
    assert!(distortion < 0.05, "strip map should be near-isometric, got distortion {distortion}");
  }

  #[test]
  fn strip_closed_ring_integer_wrap() {
    let src = "extrude_pipe(2, 4, 0..32 -> |i| { a = i / 32 * pi * 2\n v3(cos(a)*8, sin(a)*8, 0) }, \
               connect_ends=true, close_ends=false, adaptive_path_sampling=false) \
               | compute_uvs(type='strip') | render";
    let mesh = rendered_mesh(src);
    let all = uvs(&mesh);
    for uv in &all {
      assert!(uv[0].is_finite() && uv[1].is_finite(), "non-finite UV {uv:?}");
    }
    // each of the 4 ring strips is cut + rounded to an integer repeat count: every rail's max U
    // is a whole number, and no face straddles the cut with a huge U span
    let max_u = all.iter().map(|uv| uv[0]).fold(f32::MIN, f32::max);
    assert!(max_u > 10., "ring circumference ~50, expected large U extent, got {max_u}");
    assert!((max_u - max_u.round()).abs() < 1e-3, "closed strip U should be integer, got {max_u}");
    let uv_ch = &mesh.vertex_channels["uv"];
    let mut worst = 0f32;
    for (_, face) in mesh.iter_faces() {
      let (mut lo, mut hi) = (f32::MAX, f32::MIN);
      for &v in &face.vertices {
        let u = uv_ch.get(v).unwrap()[0];
        lo = lo.min(u);
        hi = hi.max(u);
      }
      worst = worst.max(hi - lo);
    }
    assert!(worst < 5., "face straddles the ring cut: U span {worst}");
  }

  #[test]
  fn strip_classifies_square_caps() {
    // 2-triangle square caps have an exact rung-score tie (diagonal walk direction); the
    // candidate-retry must classify them regardless of how trig ulps break the tie, so
    // fallback:'error' succeeds on a capped pipe.
    for src in [
      format!("{SQUARE_PIPE} | compute_uvs(type='strip', options={{ fallback: 'error' }}) | render"),
      "extrude_pipe(5, 4, 0..20 -> |i| v3(i*3, 0, 0)) \
       | compute_uvs(type='strip', options={ fallback: 'error' }) | render"
        .to_owned(),
    ] {
      crate::parse_and_eval_program(src).unwrap();
    }
  }

  #[test]
  fn strip_fallback_planar_and_error() {
    // smooth cylinder wall: one big dual-cyclic patch, not a strip
    let src = "cylinder(1, 4, 24) | compute_uvs(type='strip') | render";
    let mesh = rendered_mesh(src);
    for uv in uvs(&mesh) {
      assert!(uv[0].is_finite() && uv[1].is_finite(), "non-finite UV {uv:?}");
    }

    let err = crate::parse_and_eval_program(
      "cylinder(1, 4, 24) | compute_uvs(type='strip', options={ fallback: 'error' }) | render",
    )
    .unwrap_err();
    assert!(format!("{err}").contains("not a quad/tri strip"), "got: {err}");
  }

  /// UV inspection utility: evals `GEO_STRIP_DEBUG_SRC` (default: the verbatim square pipe) and
  /// writes every rendered mesh to `GEO_STRIP_DEBUG_DUMP` as an OBJ with `vt` UVs, openable in
  /// Blender or analyzable directly.
  /// `GEO_STRIP_DEBUG_DUMP=/tmp/strips.obj cargo test strip_debug_dump -- --nocapture`
  #[test]
  fn strip_debug_dump() {
    let Ok(out_path) = std::env::var("GEO_STRIP_DEBUG_DUMP") else {
      return;
    };
    let src = std::env::var("GEO_STRIP_DEBUG_SRC").unwrap_or_else(|_| {
      "extrude_pipe(5, 4, 0..20 -> |i| v3(i*3, sin(i * 0.1) * 10, 0)) \
       | compute_uvs(type='strip') | render"
        .to_owned()
    });
    let ctx = crate::EvalCtx::default();
    let include_prelude = std::env::var("GEO_STRIP_DEBUG_PRELUDE").is_ok();
    crate::parse_and_eval_program_with_ctx(src, &ctx, include_prelude).unwrap();
    let mut out = String::new();
    let mut base = 1usize;
    for (mi, rm) in ctx.rendered_meshes.into_inner().iter().enumerate() {
      let m = &rm.mesh.mesh;
      let uv_ch = m.vertex_channels.get("uv");
      out.push_str(&format!("o mesh{mi}\n"));
      let mut idx = fxhash::FxHashMap::default();
      for (k, v) in m.iter_vertices() {
        idx.insert(k, base + idx.len());
        let p = v.position;
        out.push_str(&format!("v {} {} {}\n", p.x, p.y, p.z));
        let uv = uv_ch.and_then(|c| c.get(k)).map(|v| [v[0], v[1]]).unwrap_or([0., 0.]);
        out.push_str(&format!("vt {} {}\n", uv[0], uv[1]));
      }
      for (_, f) in m.iter_faces() {
        let [a, b, c] = f.vertices;
        out.push_str(&format!(
          "f {}/{} {}/{} {}/{}\n",
          idx[&a], idx[&a], idx[&b], idx[&b], idx[&c], idx[&c]
        ));
      }
      base += idx.len();
    }
    std::fs::write(&out_path, out).unwrap();
    println!("wrote {out_path}");
  }

  #[test]
  fn strip_unknown_option_errors() {
    let err = crate::parse_and_eval_program(
      "box(1, 1, 1) | compute_uvs(type='strip', options={ bogus: 1 }) | render",
    )
    .unwrap_err();
    assert!(format!("{err}").contains("Unknown option"), "got: {err}");
  }
}
