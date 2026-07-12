use std::cell::RefCell;
use std::rc::Rc;

use fxhash::{FxHashMap, FxHashSet};
use mesh::{
  linked_mesh::{DisplacementNormalMethod, EdgeSplitPos, FaceKey, Vec3, Vertex, VertexKey},
  slotmap_utils::{vkey, vkey_ix},
  LinkedMesh,
};

use super::adaptive_sampler::DEFAULT_MIN_SEGMENT_LENGTH;
use crate::{autodiff, Callable, ErrorStack, EvalCtx, Value, Vec2};

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasm/cgal/cgal")]
extern "C" {
  fn cgal_get_is_loaded() -> bool;
  fn cgal_get_last_error() -> Option<String>;
  fn cgal_triangulate_polygon_2d(vertices: &[f32]) -> bool;
  fn cgal_triangulate_polygon_2d_with_holes(
    vertices: &[f32],
    subpath_lengths: &[u32],
    max_edge_len: f32,
    min_angle_bound: f32,
    refine: bool,
    interior_points: &[f32],
  ) -> bool;
  fn cgal_get_cdt2d_vertices() -> Vec<f32>;
  fn cgal_get_cdt2d_indices() -> Vec<u32>;
  fn cgal_get_cdt2d_vertex_mapping() -> Vec<i32>;
  fn cgal_clear_cdt2d_output();
}

#[derive(Clone, Copy, Debug)]
pub struct PlaneFrame {
  pub center: Vec3,
  pub u_axis: Vec3,
  pub v_axis: Vec3,
}

fn project_ring_to_2d(ring: &[Vec3], frame: &PlaneFrame) -> Vec<(f32, f32)> {
  ring
    .iter()
    .map(|p| {
      let rel = p - frame.center;
      (rel.dot(&frame.u_axis), rel.dot(&frame.v_axis))
    })
    .collect()
}

#[cfg(target_arch = "wasm32")]
fn run_triangulation(vertices: &[f32], input_vertex_count: usize) -> Result<Vec<u32>, ErrorStack> {
  crate::or_async_dep_bit(crate::DEP_BIT_CGAL);
  if !cgal_get_is_loaded() {
    return Err(ErrorStack::new_uninitialized_module("cgal"));
  }

  if !cgal_triangulate_polygon_2d(vertices) {
    let err = cgal_get_last_error().unwrap_or_else(|| "CGAL triangulation failed".to_owned());
    return Err(ErrorStack::new(err).wrap("Error triangulating polygon with CGAL"));
  }

  let out_vertices = cgal_get_cdt2d_vertices();
  let out_indices = cgal_get_cdt2d_indices();
  let vertex_mapping = cgal_get_cdt2d_vertex_mapping();
  cgal_clear_cdt2d_output();

  if out_vertices.len() % 2 != 0 {
    return Err(ErrorStack::new(format!(
      "CGAL triangulation returned invalid vertex buffer length: {}",
      out_vertices.len()
    )));
  }
  if out_indices.len() % 3 != 0 {
    return Err(ErrorStack::new(format!(
      "CGAL triangulation returned invalid triangle index count: {}",
      out_indices.len()
    )));
  }
  if vertex_mapping.len() != input_vertex_count {
    return Err(ErrorStack::new(format!(
      "CGAL triangulation returned invalid vertex mapping length: expected {input_vertex_count}, \
       found {}",
      vertex_mapping.len()
    )));
  }

  let output_vertex_count = out_vertices.len() / 2;
  let mut output_to_input: Vec<Option<u32>> = vec![None; output_vertex_count];

  for (input_ix, mapped_ix) in vertex_mapping.iter().enumerate() {
    if *mapped_ix < 0 {
      return Err(ErrorStack::new(format!(
        "CGAL triangulation returned invalid vertex mapping at input index {input_ix}",
      )));
    }
    let mapped_ix = *mapped_ix as usize;
    if mapped_ix >= output_vertex_count {
      return Err(ErrorStack::new(format!(
        "CGAL triangulation returned out-of-range vertex mapping {mapped_ix} for input index \
         {input_ix}",
      )));
    }
    if output_to_input[mapped_ix].is_none() {
      output_to_input[mapped_ix] = Some(input_ix as u32);
    }
  }

  if output_to_input.iter().any(|v| v.is_none()) {
    return Err(ErrorStack::new(
      "CGAL triangulation produced vertices that cannot be mapped to input vertices",
    ));
  }

  let mut mapped_indices = Vec::with_capacity(out_indices.len());
  for out_ix in out_indices {
    let out_ix = out_ix as usize;
    let input_ix = output_to_input
      .get(out_ix)
      .and_then(|v| *v)
      .ok_or_else(|| {
        ErrorStack::new(format!(
          "CGAL triangulation returned triangle index out of range: {out_ix}"
        ))
      })?;
    mapped_indices.push(input_ix);
  }

  Ok(mapped_indices)
}

/// Fan fill triangulation for non-wasm builds.
/// This produces topologically correct (manifold) triangulations for simple convex polygons.
/// The actual triangle geometry may be poor for concave polygons, but the topology will be valid.
#[cfg(not(target_arch = "wasm32"))]
fn run_triangulation(_vertices: &[f32], input_vertex_count: usize) -> Result<Vec<u32>, ErrorStack> {
  if input_vertex_count < 3 {
    return Err(ErrorStack::new(format!(
      "Cannot triangulate polygon with fewer than 3 vertices, found: {}",
      input_vertex_count
    )));
  }

  // Fan fill: connect vertex 0 to all other edges
  // This creates (n-2) triangles for n vertices
  let mut indices = Vec::with_capacity((input_vertex_count - 2) * 3);
  for i in 1..(input_vertex_count - 1) {
    indices.push(0u32);
    indices.push(i as u32);
    indices.push((i + 1) as u32);
  }

  Ok(indices)
}

fn tessellate_2d_polygon(points: &[(f32, f32)]) -> Result<Vec<u32>, ErrorStack> {
  if points.len() < 3 {
    return Err(ErrorStack::new(format!(
      "Cannot tessellate polygon with fewer than 3 points, found: {}",
      points.len()
    )));
  }

  let mut coords = Vec::with_capacity(points.len() * 2);
  for (x, y) in points {
    coords.push(*x);
    coords.push(*y);
  }

  let mut indices = run_triangulation(&coords, points.len())?;
  if indices.is_empty() {
    return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
  }
  // CGAL triangulation winding is opposite of the previous tessellator.
  // Flip to preserve existing winding expectations.
  for tri in indices.chunks_mut(3) {
    tri.swap(0, 2);
  }
  Ok(indices)
}

pub fn tessellate_ring_cap_with_frame(
  ring: &[Vec3],
  ring_start: usize,
  reverse_winding: bool,
  frame: &PlaneFrame,
) -> Result<Vec<u32>, ErrorStack> {
  if ring.len() < 3 {
    return Err(ErrorStack::new(format!(
      "Cannot tessellate ring with fewer than 3 vertices, found: {}",
      ring.len()
    )));
  }

  let points_2d = project_ring_to_2d(ring, frame);
  let local_indices = tessellate_2d_polygon(&points_2d)?;

  let indices: Vec<u32> = if reverse_winding {
    local_indices
      .chunks(3)
      .flat_map(|tri| {
        [
          tri[2] + ring_start as u32,
          tri[1] + ring_start as u32,
          tri[0] + ring_start as u32,
        ]
      })
      .collect()
  } else {
    local_indices
      .iter()
      .map(|&idx| idx + ring_start as u32)
      .collect()
  };

  Ok(indices)
}

/// Tessellates a 3D ring using existing vertex indices.
///
/// `ring` supplies positions in winding order, and `ring_indices` maps each position to an
/// existing vertex index in the mesh.  This allows caps to be generated without duplicating
/// vertices.
pub fn tessellate_ring_cap_with_indices(
  ring: &[Vec3],
  ring_indices: &[u32],
  reverse_winding: bool,
  frame: &PlaneFrame,
) -> Result<Vec<u32>, ErrorStack> {
  if ring.len() < 3 {
    return Err(ErrorStack::new(format!(
      "Cannot tessellate ring with fewer than 3 vertices, found: {}",
      ring.len()
    )));
  }
  if ring.len() != ring_indices.len() {
    return Err(ErrorStack::new(
      "Ring indices length must match ring vertex length",
    ));
  }

  let points_2d = project_ring_to_2d(ring, frame);
  let local_indices = tessellate_2d_polygon(&points_2d)?;

  let indices: Vec<u32> = if reverse_winding {
    local_indices
      .chunks(3)
      .flat_map(|tri| {
        [
          ring_indices[tri[2] as usize],
          ring_indices[tri[1] as usize],
          ring_indices[tri[0] as usize],
        ]
      })
      .collect()
  } else {
    local_indices
      .iter()
      .map(|&idx| ring_indices[idx as usize])
      .collect()
  };

  Ok(indices)
}

/// Tessellates a cap over a group of nested profile loops (one outer plus its holes) reusing the
/// existing shared mesh vertices, so the cap is watertight with the swept walls.
///
/// `loops[k]` is loop k's 3D vertices in sampling order; `loop_base_indices[k]` is the flat mesh
/// vertex index of loop k's first vertex. Loops must be disjoint and wound to the outer-CCW /
/// hole-CW convention (CGAL classifies the filled region by nesting parity). Available only in
/// wasm builds (native CGAL supports a single subpath).
pub fn tessellate_ring_cap_with_holes(
  loops: &[&[Vec3]],
  loop_base_indices: &[u32],
  reverse_winding: bool,
  frame: &PlaneFrame,
) -> Result<Vec<u32>, ErrorStack> {
  if loops.len() != loop_base_indices.len() {
    return Err(ErrorStack::new(
      "loop count must match loop_base_indices length",
    ));
  }

  // Concatenate all loops' projected 2D coords; remember each input vertex's shared mesh index.
  let mut coords: Vec<f32> = Vec::new();
  let mut subpath_lengths: Vec<u32> = Vec::with_capacity(loops.len());
  let mut input_to_flat: Vec<u32> = Vec::new();
  for (ring, &base) in loops.iter().zip(loop_base_indices) {
    if ring.len() < 3 {
      return Err(ErrorStack::new(format!(
        "Cannot cap a loop with fewer than 3 vertices, found: {}",
        ring.len()
      )));
    }
    for (i, p) in ring.iter().enumerate() {
      let rel = *p - frame.center;
      coords.push(rel.dot(&frame.u_axis));
      coords.push(rel.dot(&frame.v_axis));
      input_to_flat.push(base + i as u32);
    }
    subpath_lengths.push(ring.len() as u32);
  }

  let input_vertex_count = input_to_flat.len();
  let (out_vertices_xy, out_indices, vertex_mapping) =
    run_triangulation_with_holes(&coords, &subpath_lengths, CgalCdtOptions::default(), &[])?;
  if out_indices.is_empty() {
    return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
  }
  if vertex_mapping.len() != input_vertex_count {
    return Err(ErrorStack::new(format!(
      "CGAL triangulation returned invalid vertex mapping length: expected {input_vertex_count}, \
       found {}",
      vertex_mapping.len()
    )));
  }

  // Invert input→output into output→shared-index (first input wins for coincident points), then
  // map every output triangle index straight back to a shared mesh vertex.
  let output_vertex_count = out_vertices_xy.len() / 2;
  let mut output_to_flat: Vec<Option<u32>> = vec![None; output_vertex_count];
  for (input_ix, &mapped) in vertex_mapping.iter().enumerate() {
    if mapped < 0 || mapped as usize >= output_vertex_count {
      return Err(ErrorStack::new(format!(
        "CGAL triangulation returned invalid vertex mapping {mapped} for input index {input_ix}"
      )));
    }
    let slot = &mut output_to_flat[mapped as usize];
    if slot.is_none() {
      *slot = Some(input_to_flat[input_ix]);
    }
  }

  let mut indices: Vec<u32> = Vec::with_capacity(out_indices.len());
  for &o in &out_indices {
    let flat = output_to_flat
      .get(o as usize)
      .and_then(|v| *v)
      .ok_or_else(|| ErrorStack::new(format!("CGAL cap triangle index out of range: {o}")))?;
    indices.push(flat);
  }

  // The single-polygon cap's CGAL-winding correction and `reverse_winding` cancel to one pass.
  if !reverse_winding {
    for tri in indices.chunks_mut(3) {
      tri.swap(0, 2);
    }
  }

  Ok(indices)
}

/// Coordinate-plane embedding for a 2D tessellation, given as a two-axis swizzle: the 2D `(u, v)`
/// maps to `u_axis` and `v_axis` respectively (the remaining axis = 0).  Order matters — "xz" and
/// "zx" are mirror embeddings.  The default front face points along the +remaining axis; `flipped`
/// reverses it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TessPlane {
  u_axis: u8,
  v_axis: u8,
}

impl Default for TessPlane {
  fn default() -> Self {
    TessPlane {
      u_axis: 0,
      v_axis: 2,
    }
  }
}

impl TessPlane {
  pub fn parse(s: &str) -> Option<Self> {
    let axis = |b: u8| match b.to_ascii_lowercase() {
      b'x' => Some(0u8),
      b'y' => Some(1),
      b'z' => Some(2),
      _ => None,
    };
    let bytes = s.as_bytes();
    if bytes.len() != 2 {
      return None;
    }
    let u_axis = axis(bytes[0])?;
    let v_axis = axis(bytes[1])?;
    if u_axis == v_axis {
      return None;
    }
    Some(TessPlane { u_axis, v_axis })
  }

  fn embed(self, u: f32, v: f32) -> Vec3 {
    let mut p = Vec3::zeros();
    p[self.u_axis as usize] = u;
    p[self.v_axis as usize] = v;
    p
  }

  /// Whether a param-space CCW triangle embedded here already faces the +remaining axis (true iff
  /// `(u_axis, v_axis, remaining)` is an even permutation of `(x, y, z)`).
  fn ccw_faces_positive(self) -> bool {
    (self.v_axis + 3 - self.u_axis) % 3 == 1
  }
}

pub fn tessellate_2d_paths(
  paths: &[Vec<Vec2>],
  flipped: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  tessellate_2d_paths_multi(
    paths,
    flipped,
    TessPlane::default(),
    CgalCdtOptions::default(),
  )
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CgalCdtOptions {
  /// Upper bound on triangle edge length.  Triangles with any edge longer than this are split
  /// by the Delaunay mesher.
  pub max_edge_len: Option<f32>,
  /// Aspect bound passed to `Delaunay_mesh_size_criteria_2` — squared sine of the minimum
  /// allowed angle.  0.125 (≈ 20.6°) is the CGAL default and the upper limit at which
  /// termination is provably guaranteed.  Setting this explicitly to `Some(0.0)` disables the
  /// shape criterion entirely (size-only refinement).
  pub min_angle_squared_sine: Option<f32>,
}

impl CgalCdtOptions {
  /// Refinement is requested when either constraint is set; if both are `None`, the raw CDT
  /// is returned and the strict input-vertex-to-output-vertex mapping is preserved.
  pub fn refine(&self) -> bool {
    self.max_edge_len.is_some() || self.min_angle_squared_sine.is_some()
  }
}

#[cfg(target_arch = "wasm32")]
fn run_triangulation_with_holes(
  vertices: &[f32],
  subpath_lengths: &[u32],
  options: CgalCdtOptions,
  interior_points: &[f32],
) -> Result<(Vec<f32>, Vec<u32>, Vec<i32>), ErrorStack> {
  crate::or_async_dep_bit(crate::DEP_BIT_CGAL);
  if !cgal_get_is_loaded() {
    return Err(ErrorStack::new_uninitialized_module("cgal"));
  }

  let refine = options.refine();
  // CGAL treats 0 as "no size constraint"; aspect_bound defaults to 0.125 when not specified
  // by the user.  Both are ignored when `refine == false`.
  let max_edge_len = options.max_edge_len.unwrap_or(0.0);
  let min_angle_squared_sine = options.min_angle_squared_sine.unwrap_or(0.125);

  if !cgal_triangulate_polygon_2d_with_holes(
    vertices,
    subpath_lengths,
    max_edge_len,
    min_angle_squared_sine,
    refine,
    interior_points,
  ) {
    let err =
      cgal_get_last_error().unwrap_or_else(|| "CGAL multi-subpath triangulation failed".to_owned());
    return Err(ErrorStack::new(err).wrap("Error triangulating multi-subpath polygon with CGAL"));
  }

  let out_vertices = cgal_get_cdt2d_vertices();
  let out_indices = cgal_get_cdt2d_indices();
  // Non-empty only in the non-refining case, where each input vertex has an output correspondent.
  let vertex_mapping = cgal_get_cdt2d_vertex_mapping();
  cgal_clear_cdt2d_output();

  if out_vertices.len() % 2 != 0 {
    return Err(ErrorStack::new(format!(
      "CGAL triangulation returned invalid vertex buffer length: {}",
      out_vertices.len()
    )));
  }
  if out_indices.len() % 3 != 0 {
    return Err(ErrorStack::new(format!(
      "CGAL triangulation returned invalid triangle index count: {}",
      out_indices.len()
    )));
  }

  Ok((out_vertices, out_indices, vertex_mapping))
}

#[cfg(not(target_arch = "wasm32"))]
fn run_triangulation_with_holes(
  vertices: &[f32],
  subpath_lengths: &[u32],
  options: CgalCdtOptions,
  interior_points: &[f32],
) -> Result<(Vec<f32>, Vec<u32>, Vec<i32>), ErrorStack> {
  // Native builds only support the single-subpath, non-refining case, matching the legacy
  // fan-fill fallback that backed `tessellate_2d_paths`.  Real CDT, Delaunay refinement, and
  // interior-point insertion live in the CGAL wasm — except under `cargo test`, where a single
  // subpath's refine/interior case is served by `spade` so the embed_path refinement path is
  // exercisable natively.
  #[cfg(test)]
  if subpath_lengths.len() == 1 && (options.refine() || !interior_points.is_empty()) {
    return native_tests::spade_refined_triangulation(vertices, options, interior_points);
  }
  if subpath_lengths.len() != 1 || options.refine() || !interior_points.is_empty() {
    return Err(ErrorStack::new(
      "Multi-subpath / refining / interior-point CGAL triangulation is only available in wasm \
       builds",
    ));
  }
  let vertex_count = subpath_lengths[0] as usize;
  let indices = run_triangulation(vertices, vertex_count)?;
  // The fan fill preserves input vertices verbatim, so the mapping is the identity.
  let vertex_mapping = (0..vertex_count as i32).collect();
  Ok((vertices.to_vec(), indices, vertex_mapping))
}

pub fn tessellate_2d_paths_multi(
  paths: &[Vec<Vec2>],
  flipped: bool,
  plane: TessPlane,
  options: CgalCdtOptions,
) -> Result<LinkedMesh<()>, ErrorStack> {
  // `_embedded`'s default front faces −(φ_u × φ_v); xor with the plane's parity to keep this
  // function's "+remaining axis" default.
  let (mesh, _domain_uvs) =
    tessellate_2d_paths_embedded(paths, plane.ccw_faces_positive() ^ flipped, options, |uv| {
      Ok(plane.embed(uv.x, uv.y))
    })?;
  Ok(mesh)
}

/// Validates (>= 3 points per subpath) and flattens `paths` into the flat coord + subpath-length
/// buffers CGAL's multi-subpath CDT consumes.
fn flatten_paths(paths: &[Vec<Vec2>]) -> Result<(Vec<f32>, Vec<u32>), ErrorStack> {
  let mut coords: Vec<f32> = Vec::with_capacity(paths.iter().map(Vec::len).sum::<usize>() * 2);
  let mut subpath_lengths: Vec<u32> = Vec::with_capacity(paths.len());
  for (ix, path) in paths.iter().enumerate() {
    if path.len() < 3 {
      return Err(ErrorStack::new(format!(
        "Cannot tessellate subpath {ix} with fewer than 3 points, found: {}",
        path.len()
      )));
    }
    for pt in path {
      coords.push(pt.x);
      coords.push(pt.y);
    }
    subpath_lengths.push(path.len() as u32);
  }
  Ok((coords, subpath_lengths))
}

/// Constrained-Delaunay-triangulates `paths` (outer + holes via subpath nesting), then embeds each
/// output 2D vertex into 3D through `embed`.  This is `tessellate_2d_paths_multi` generalized from
/// an affine `TessPlane` to an arbitrary map φ: ℝ²→ℝ³.  The default front face points along
/// −(φ_u × φ_v) — for a flat `|p| (p.x, 0, p.y)`-style embedding that's +Y, matching
/// `tessellate_2d_paths_multi`'s legacy XZ default; `flipped` swaps it.  The resulting single-layer
/// cap is what `embed_path` thickens.  Also returns each output vertex's 2D domain coordinate,
/// aligned with the mesh's vertex order (`vkey(i+1, 1)`), so callers can recover φ's frame per
/// vertex.
pub fn tessellate_2d_paths_embedded(
  paths: &[Vec<Vec2>],
  flipped: bool,
  options: CgalCdtOptions,
  embed: impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
) -> Result<(LinkedMesh<()>, Vec<Vec2>), ErrorStack> {
  if paths.is_empty() {
    return Ok((LinkedMesh::default(), Vec::new()));
  }

  let (coords, subpath_lengths) = flatten_paths(paths)?;
  let (out_vertices_xy, mut indices, _vertex_mapping) =
    run_triangulation_with_holes(&coords, &subpath_lengths, options, &[])?;
  if indices.is_empty() {
    return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
  }

  let mut verts: Vec<Vec3> = Vec::with_capacity(out_vertices_xy.len() / 2);
  let mut domain_uvs: Vec<Vec2> = Vec::with_capacity(out_vertices_xy.len() / 2);
  for xy in out_vertices_xy.chunks_exact(2) {
    let uv = Vec2::new(xy[0], xy[1]);
    verts.push(embed(uv)?);
    domain_uvs.push(uv);
  }

  // CGAL emits CCW-in-param triangles, which face +(φ_u × φ_v); swap to the −(φ_u × φ_v) default.
  if !flipped {
    for tri in indices.chunks_mut(3) {
      tri.swap(0, 2);
    }
  }

  Ok((
    LinkedMesh::from_indexed_vertices(&verts, &indices, None, None),
    domain_uvs,
  ))
}

const DENSIFY_MAX_DEPTH: u32 = 12;

/// A domain point sampled on both surfaces sharing the cap boundary: the top cap φ(uv) and the
/// offset cap φ(uv) ∓ t·N(uv).  When no offset surface is tracked `off == top`, so every deviation
/// test degenerates to single-surface behavior for free.
#[derive(Clone, Copy)]
struct SurfPt {
  top: Vec3,
  off: Vec3,
}

/// Which probed surface a refinement pass measures (and which one positions its mesh).  `Both`
/// (max deviation, top positions) is the shared-CDT behavior; `Top`/`Off` drive the per-cap
/// independent interiors of [`tessellate_2d_paths_embedded_two_caps`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SurfSel {
  Both,
  Top,
  Off,
}

impl SurfPt {
  fn dev_from(self, chord: SurfPt, sel: SurfSel) -> f32 {
    let t = (self.top - chord.top).norm();
    let o = (self.off - chord.off).norm();
    match sel {
      SurfSel::Both => t.max(o),
      SurfSel::Top => t,
      SurfSel::Off => o,
    }
  }

  fn pos(self, sel: SurfSel) -> Vec3 {
    if sel == SurfSel::Off {
      self.off
    } else {
      self.top
    }
  }
}

impl std::ops::Add for SurfPt {
  type Output = SurfPt;
  fn add(self, o: SurfPt) -> SurfPt {
    SurfPt {
      top: self.top + o.top,
      off: self.off + o.off,
    }
  }
}

impl std::ops::Mul<f32> for SurfPt {
  type Output = SurfPt;
  fn mul(self, s: f32) -> SurfPt {
    SurfPt {
      top: self.top * s,
      off: self.off * s,
    }
  }
}

/// Guard-aware recursive chord-flattening of the 2D segment `[a, b]` (with pre-probed endpoints
/// `ea`/`eb` and endpoint guard values `ga`/`gb`): appends strictly-interior subdivision points so
/// the probed surfaces track their true shape within `tol` — the deviation of the true midpoint
/// from the chord midpoint, on whichever surface is worse.
///
/// A clear guard sign flip splits at the *crease* (bisected root) instead of the midpoint, so a C¹
/// break is consumed as an exact vertex rather than chased as curvature; recursion then continues
/// on the smooth halves.  Standard midpoint flattening applies otherwise.
#[allow(clippy::too_many_arguments)]
fn densify_segment_under_embed(
  a: Vec2,
  b: Vec2,
  ea: SurfPt,
  eb: SurfPt,
  ga: Option<Rc<Vec<f32>>>,
  gb: Option<Rc<Vec<f32>>>,
  probe: &impl Fn(Vec2) -> Result<SurfPt, ErrorStack>,
  guards: Option<GuardEval>,
  tol: f32,
  sel: SurfSel,
  depth: u32,
  out: &mut Vec<Vec2>,
  crease: &mut CreaseUvs,
) -> Result<(), ErrorStack> {
  if depth >= DENSIFY_MAX_DEPTH || (b - a).norm() <= DEFAULT_MIN_SEGMENT_LENGTH {
    return Ok(());
  }
  if let (Some(gav), Some(gbv), Some(g)) = (&ga, &gb, guards) {
    for k in 0..gav.len().min(gbv.len()) {
      if !clear_straddle(gav[k], gbv[k]) {
        continue;
      }
      let Some(root) = bisect_guard_root(a, b, 0., 1., k, g) else {
        continue;
      };
      if (root - a).norm() <= DEFAULT_MIN_SEGMENT_LENGTH
        || (b - root).norm() <= DEFAULT_MIN_SEGMENT_LENGTH
      {
        continue;
      }
      let eroot = probe(root)?;
      let groot = g(root).map(Rc::new);
      record_crease(crease, root, k);
      densify_segment_under_embed(
        a,
        root,
        ea,
        eroot,
        ga,
        groot.clone(),
        probe,
        guards,
        tol,
        sel,
        depth + 1,
        out,
        crease,
      )?;
      out.push(root);
      densify_segment_under_embed(
        root,
        b,
        eroot,
        eb,
        groot,
        gb,
        probe,
        guards,
        tol,
        sel,
        depth + 1,
        out,
        crease,
      )?;
      return Ok(());
    }
  }
  let mid = (a + b) * 0.5;
  let emid = probe(mid)?;
  if emid.dev_from((ea + eb) * 0.5, sel) <= tol {
    return Ok(());
  }
  let gmid = match guards {
    Some(g) if ga.is_some() => g(mid).map(Rc::new),
    _ => None,
  };
  densify_segment_under_embed(
    a,
    mid,
    ea,
    emid,
    ga,
    gmid.clone(),
    probe,
    guards,
    tol,
    sel,
    depth + 1,
    out,
    crease,
  )?;
  out.push(mid);
  densify_segment_under_embed(
    mid,
    b,
    emid,
    eb,
    gmid,
    gb,
    probe,
    guards,
    tol,
    sel,
    depth + 1,
    out,
    crease,
  )?;
  Ok(())
}

/// Densifies a closed boundary loop so its image under the probed surface(s) deviates from the true
/// shape by at most `tol`.  Each input vertex is kept (constrained feature points ride through);
/// interior points are inserted per segment, including the wrap segment.
fn densify_loop_under_embed(
  loop_2d: &[Vec2],
  probe: &impl Fn(Vec2) -> Result<SurfPt, ErrorStack>,
  guards: Option<GuardEval>,
  tol: f32,
  sel: SurfSel,
  crease: &mut CreaseUvs,
) -> Result<Vec<Vec2>, ErrorStack> {
  let n = loop_2d.len();
  let mut out = Vec::with_capacity(n * 2);
  let mut probed: Vec<SurfPt> = Vec::with_capacity(n);
  let mut gvals: Vec<Option<Rc<Vec<f32>>>> = Vec::with_capacity(n);
  for &p in loop_2d {
    probed.push(probe(p)?);
    gvals.push(guards.and_then(|g| g(p)).map(Rc::new));
  }
  for k in 0..n {
    let j = (k + 1) % n;
    out.push(loop_2d[k]);
    densify_segment_under_embed(
      loop_2d[k],
      loop_2d[j],
      probed[k],
      probed[j],
      gvals[k].clone(),
      gvals[j].clone(),
      probe,
      guards,
      tol,
      sel,
      0,
      &mut out,
      crease,
    )?;
  }
  Ok(out)
}

/// Max deviation of the flat triangle `(a, b, c)` from the true probed surface(s), measured at the
/// three edge midpoints and the centroid.
#[allow(clippy::too_many_arguments)]
fn embedded_triangle_deviation(
  a2: Vec2,
  b2: Vec2,
  c2: Vec2,
  a3: SurfPt,
  b3: SurfPt,
  c3: SurfPt,
  probe: Probe,
  sel: SurfSel,
) -> Result<f32, ErrorStack> {
  let mut dev = 0f32;
  for (m2, chord) in [
    ((a2 + b2) * 0.5, (a3 + b3) * 0.5),
    ((b2 + c2) * 0.5, (b3 + c3) * 0.5),
    ((c2 + a2) * 0.5, (c3 + a3) * 0.5),
    ((a2 + b2 + c2) / 3.0, (a3 + b3 + c3) * (1. / 3.)),
  ] {
    dev = dev.max(probe(m2)?.dev_from(chord, sel));
  }
  Ok(dev)
}

/// Converges by ≤25 iterations for the cases exercised so far, early-terminating at convergence;
/// the cap only bites on pathological φ.  Kept generous because a too-low cap silently returns an
/// under-refined mesh (over-tolerance triangles, no warning) rather than erroring.
const REFINE_MAX_ITERS: u32 = 30;

/// Evaluates the crease guard functions (design §12) at a domain point; a fixed-length vector of
/// switching values whose sign changes mark C¹ discontinuities of the probed surfaces.  `None`
/// once guards are disabled (evaluation error) — the caller falls back to plain refinement.
pub type GuardEval<'a> = &'a dyn Fn(Vec2) -> Option<Vec<f32>>;

/// How refinement ended; consumed by `embed_path` to warn on under-refinement.
#[derive(Clone, Copy, Default, Debug)]
pub struct RefineStats {
  /// Over-tolerance triangles in the final iteration that contributed no new point (min-edge
  /// floor, or every candidate insertion was a near-duplicate) — i.e. left unresolved.
  pub stalled: usize,
  pub budget_hit: bool,
  pub converged: bool,
}

const GUARD_BOUNDARY_SCAN_SAMPLES: usize = 8;
const GUARD_BISECT_ITERS: u32 = 24;
/// Rung-0 safety valve: hard cap on accumulated interior Steiner points.
const REFINE_POINT_BUDGET: usize = 50_000;
/// Rung-0 floor: over-tol triangles whose longest domain edge is below this fraction of the
/// domain bbox diagonal are left alone (stalled) rather than split forever.  A balance: small
/// enough that legitimate fine features on large domains still resolve (smooth features converge
/// quadratically and rarely approach it), large enough that unresolvable value-jump bands stall
/// well before the point budget.
const MIN_SPLIT_EDGE_FRAC: f32 = 3e-4;
const DEDUP_RADIUS_FRAC: f32 = 1e-5;

/// Near-duplicate filter for queued Steiner points: a vertex sitting on a crease has guard ≈ 0, so
/// adjacent edges can show hairline sign flips whose roots land right back on it.
struct DedupGrid {
  radius_sq: f32,
  inv_cell: f32,
  cells: FxHashMap<(i32, i32), Vec<Vec2>>,
}

impl DedupGrid {
  fn new(radius: f32) -> Self {
    DedupGrid {
      radius_sq: radius * radius,
      inv_cell: 1. / radius,
      cells: FxHashMap::default(),
    }
  }

  fn try_insert(&mut self, p: Vec2) -> bool {
    let (kx, ky) = (
      (p.x * self.inv_cell).floor() as i32,
      (p.y * self.inv_cell).floor() as i32,
    );
    for dx in -1..=1 {
      for dy in -1..=1 {
        if let Some(pts) = self.cells.get(&(kx + dx, ky + dy)) {
          if pts.iter().any(|q| (q - p).norm_squared() < self.radius_sq) {
            return false;
          }
        }
      }
    }
    self.cells.entry((kx, ky)).or_default().push(p);
    true
  }
}

/// A *clear* sign straddle: opposite signs AND both magnitudes well away from zero relative to
/// each other.  Vertices sitting on a crease carry `g ≈ ±ε` bisection residue, so a plain
/// `ga·gb < 0` test fires on edges *along* the crease and bisection then inserts junk points
/// (root-finding on noise).  The relative floor rejects those while keeping genuine crossings.
fn clear_straddle(ga: f32, gb: f32) -> bool {
  ga * gb < 0. && ga.abs().min(gb.abs()) > 1e-3 * ga.abs().max(gb.abs())
}

/// Domain points inserted on guard zero sets, with a bitmask of which guards vanish there.
/// Consumed by `embed_path` to sharp-mark crease edges.
pub type CreaseUvs = FxHashMap<[u32; 2], u16>;

fn record_crease(crease: &mut CreaseUvs, p: Vec2, g_ix: usize) {
  *crease.entry([p.x.to_bits(), p.y.to_bits()]).or_insert(0) |= 1 << g_ix.min(15);
}

/// Bisects guard `g_ix` to its zero on the domain segment `a→b` within param bracket `[lo, hi]`
/// (which must straddle the zero).  Returns the root point.
fn bisect_guard_root(
  a: Vec2,
  b: Vec2,
  mut lo: f32,
  mut hi: f32,
  g_ix: usize,
  guards: GuardEval,
) -> Option<Vec2> {
  let g_at = |t: f32| guards(a + (b - a) * t).and_then(|v| v.get(g_ix).copied());
  let mut glo = g_at(lo)?;
  for _ in 0..GUARD_BISECT_ITERS {
    let mid = (lo + hi) * 0.5;
    let gm = g_at(mid)?;
    if glo * gm <= 0. {
      hi = mid;
    } else {
      lo = mid;
      glo = gm;
    }
  }
  Some(a + (b - a) * ((lo + hi) * 0.5))
}

/// The 1D critical-t case of §12 rung 1: root-finds every guard along each boundary segment and
/// splices the roots in as mandatory vertices, so the densifier samples *between* creases and the
/// CDT gets constrained boundary vertices exactly on them.  (The recursive densifier is also
/// guard-aware, catching crossings this coarse scan misses.)  Falls back to the original loop on
/// any guard failure.
fn insert_boundary_guard_roots(
  loop_2d: &[Vec2],
  guards: GuardEval,
  crease: &mut CreaseUvs,
) -> Option<Vec<Vec2>> {
  let n = loop_2d.len();
  let mut out = Vec::with_capacity(n * 2);
  for k in 0..n {
    let (a, b) = (loop_2d[k], loop_2d[(k + 1) % n]);
    out.push(a);
    let mut roots: Vec<(f32, usize)> = Vec::new();
    let mut prev = guards(a)?;
    for i in 1..=GUARD_BOUNDARY_SCAN_SAMPLES {
      let t = i as f32 / GUARD_BOUNDARY_SCAN_SAMPLES as f32;
      let cur = guards(a + (b - a) * t)?;
      for g in 0..prev.len().min(cur.len()) {
        if clear_straddle(prev[g], cur[g]) {
          if let Some(root) = bisect_guard_root(
            a,
            b,
            t - 1. / GUARD_BOUNDARY_SCAN_SAMPLES as f32,
            t,
            g,
            guards,
          ) {
            let rt = (root - a).norm() / (b - a).norm().max(1e-12);
            roots.push((rt, g));
          }
        }
      }
      prev = cur;
    }
    roots.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap());
    let mut last = 0f32;
    for (rt, g) in roots {
      if rt > last + 1e-3 && rt < 1. - 1e-3 {
        let p = a + (b - a) * rt;
        record_crease(crease, p, g);
        out.push(p);
        last = rt;
      }
    }
  }
  Some(out)
}

/// Min-angle quality floor for the refined cap's CDT (squared sine).  The deviation-only refiner
/// leaves locally-flat slivers (e.g. a tall base whose embedded height doesn't vary along it);
/// enforcing this kills them.  0.11 ≈ sin²(19.5°), just under CGAL's 20.7° termination guarantee.
const REFINE_MIN_ANGLE_SQ_SINE: f32 = 0.11;

/// Distortion-aware version of `tessellate_2d_paths_embedded`: densifies each boundary loop under φ
/// to `tol`, then refines the interior a-posteriori — constrained-Delaunay-triangulate → embed →
/// drop a Steiner point at the centroid of every triangle still deviating from φ by more than `tol`
/// → re-triangulate with the accumulated points → repeat until converged (or the iteration cap).
/// The CDT itself enforces a min-angle quality floor ([`REFINE_MIN_ANGLE_SQ_SINE`]) so slivers
/// can't survive in regions the deviation test leaves alone.  Spatially adaptive: flat regions stay
/// coarse, curved regions densify where they exceed tolerance.
///
/// `offset_surface`, when given, is the second surface sharing this CDT (the offset cap
/// `φ(uv) ∓ t·N(uv)`); both boundary densification and interior refinement then split on the worse
/// of the two deviations, so a spatially-varying thickness that curves the offset cap gets resolved
/// even where φ itself is flat.
///
/// `guards`, when given, enables crease-aware refinement (§12 rungs 0+1): boundary segments get
/// mandatory vertices at guard roots, and over-tolerance triangles straddling a guard sign change
/// insert their Steiner point *on* the crease (edge bisection) instead of at the centroid, so
/// C¹-discontinuous fields converge instead of piling points into the crease band.
pub fn tessellate_2d_paths_embedded_refined(
  paths: &[Vec<Vec2>],
  flipped: bool,
  tol: f32,
  embed: impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
  offset_surface: Option<&dyn Fn(Vec2) -> Result<Vec3, ErrorStack>>,
  guards: Option<GuardEval>,
) -> Result<(LinkedMesh<()>, Vec<Vec2>, RefineStats, CreaseUvs), ErrorStack> {
  let mut crease = CreaseUvs::default();
  if paths.is_empty() {
    return Ok((
      LinkedMesh::default(),
      Vec::new(),
      RefineStats::default(),
      crease,
    ));
  }

  // Memoized like the per-run guard caches: vertex/probe positions repeat across
  // re-triangulations, and each uncached probe costs several interpreter invocations (embed + FD
  // frame + thickness).
  let probe_cache: RefCell<FxHashMap<[u32; 2], SurfPt>> = RefCell::new(FxHashMap::default());
  let probe = |uv: Vec2| -> Result<SurfPt, ErrorStack> {
    let key = [uv.x.to_bits(), uv.y.to_bits()];
    if let Some(&hit) = probe_cache.borrow().get(&key) {
      return Ok(hit);
    }
    let top = embed(uv)?;
    let off = match offset_surface {
      Some(f) => f(uv)?,
      None => top,
    };
    let pt = SurfPt { top, off };
    probe_cache.borrow_mut().insert(key, pt);
    Ok(pt)
  };

  let dense = paths
    .iter()
    .map(|path| {
      let with_roots = guards
        .and_then(|g| insert_boundary_guard_roots(path, g, &mut crease))
        .unwrap_or_else(|| path.clone());
      densify_loop_under_embed(&with_roots, &probe, guards, tol, SurfSel::Both, &mut crease)
    })
    .collect::<Result<Vec<_>, _>>()?;
  let (coords, subpath_lengths) = flatten_paths(&dense)?;
  let (min_split_edge, dedup_radius) = domain_scales(&coords);
  let env = RefineEnv {
    probe: &probe,
    coords: &coords,
    subpath_lengths: &subpath_lengths,
    flipped,
    tol,
    min_split_edge,
    dedup_radius,
  };
  let (mesh, domain_uvs, stats) = refine_cap(&env, SurfSel::Both, guards, &mut crease)?;
  Ok((mesh, domain_uvs, stats, crease))
}

/// Rung-0 floor + dedup radius derived from the densified boundary's bbox diagonal.
fn domain_scales(coords: &[f32]) -> (f32, f32) {
  let (mut lo, mut hi) = (Vec2::repeat(f32::INFINITY), Vec2::repeat(f32::NEG_INFINITY));
  for c in coords.chunks_exact(2) {
    let p = Vec2::new(c[0], c[1]);
    lo = lo.inf(&p);
    hi = hi.sup(&p);
  }
  let bbox_diag = (hi - lo).norm();
  (
    bbox_diag * MIN_SPLIT_EDGE_FRAC,
    (bbox_diag * DEDUP_RADIUS_FRAC).max(DEFAULT_MIN_SEGMENT_LENGTH),
  )
}

type Probe<'a> = &'a dyn Fn(Vec2) -> Result<SurfPt, ErrorStack>;

/// Everything shared between the refinement passes of one tessellation: the (memoizing) surface
/// probe and the densified boundary.
struct RefineEnv<'a> {
  probe: Probe<'a>,
  coords: &'a [f32],
  subpath_lengths: &'a [u32],
  flipped: bool,
  tol: f32,
  min_split_edge: f32,
  dedup_radius: f32,
}

/// One a-posteriori interior refinement pass over the shared boundary (see
/// [`tessellate_2d_paths_embedded_refined`]'s docs for the loop's semantics), measuring the
/// surface(s) selected by `sel` and positioning the returned mesh on `sel`'s primary surface.
/// Includes the crease-alignment post-pass.
fn refine_cap(
  env: &RefineEnv,
  sel: SurfSel,
  guards: Option<GuardEval>,
  crease: &mut CreaseUvs,
) -> Result<(LinkedMesh<()>, Vec<Vec2>, RefineStats), ErrorStack> {
  let mut stats = RefineStats::default();
  let probe = env.probe;
  let (coords, subpath_lengths, tol) = (env.coords, env.subpath_lengths, env.tol);

  let mut dedup = DedupGrid::new(env.dedup_radius);
  for c in coords.chunks_exact(2) {
    dedup.try_insert(Vec2::new(c[0], c[1]));
  }
  // Per-pass: different passes may see different guard subsets, so cached values can't cross.
  let mut guard_cache: FxHashMap<[u32; 2], Option<Rc<Vec<f32>>>> = FxHashMap::default();

  // Quality-refining CDT: min-angle Steiner points split slivers and long boundary edges that the
  // deviation test alone can't reach.  Native `cargo test` serves this via `spade` (see
  // `run_triangulation_with_holes`); production tessellates it with CGAL/wasm.
  let cdt_options = CgalCdtOptions {
    max_edge_len: None,
    min_angle_squared_sine: Some(REFINE_MIN_ANGLE_SQ_SINE),
  };

  let mut interior: Vec<f32> = Vec::new();
  let mut result: Option<(Vec<Vec2>, Vec<SurfPt>, Vec<u32>)> = None;

  for _ in 0..REFINE_MAX_ITERS {
    let (out_xy, indices, _mapping) =
      run_triangulation_with_holes(coords, subpath_lengths, cdt_options, &interior)?;
    if indices.is_empty() {
      return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
    }

    let verts_2d: Vec<Vec2> = out_xy
      .chunks_exact(2)
      .map(|xy| Vec2::new(xy[0], xy[1]))
      .collect();
    let verts_3d: Vec<SurfPt> = verts_2d
      .iter()
      .map(|&p| probe(p))
      .collect::<Result<_, _>>()?;

    // Lazily evaluated + memoized by position bits so values survive re-triangulation (vertex
    // order changes across iterations, positions mostly don't).
    let mut guard_at = |uv: Vec2| -> Option<Rc<Vec<f32>>> {
      let g = guards?;
      guard_cache
        .entry([uv.x.to_bits(), uv.y.to_bits()])
        .or_insert_with(|| g(uv).map(Rc::new))
        .clone()
    };

    let mut fresh: Vec<f32> = Vec::new();
    stats.stalled = 0;
    for tri in indices.chunks_exact(3) {
      let (i, j, k) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
      let dev = embedded_triangle_deviation(
        verts_2d[i],
        verts_2d[j],
        verts_2d[k],
        verts_3d[i],
        verts_3d[j],
        verts_3d[k],
        probe,
        sel,
      )?;
      if dev <= tol {
        continue;
      }

      // Rung 1: a clear guard sign change across an edge means the crease runs through this
      // triangle — put the Steiner point exactly on it so on-crease points chain into aligned
      // edges.  A straddling triangle whose roots all dedup away gets NO fallback point: the
      // crease vertices already exist, and an off-crease centroid would only add clutter for the
      // Delaunay pass to fight (the spiderweb-knot failure mode).
      let mut inserted = false;
      let mut straddling = false;
      if let (Some(gi), Some(gj), Some(gk)) = (
        guard_at(verts_2d[i]),
        guard_at(verts_2d[j]),
        guard_at(verts_2d[k]),
      ) {
        let corners = [(verts_2d[i], &gi), (verts_2d[j], &gj), (verts_2d[k], &gk)];
        for e in 0..3 {
          let (pa, ga) = corners[e];
          let (pb, gb) = corners[(e + 1) % 3];
          for g_ix in 0..ga.len().min(gb.len()) {
            if clear_straddle(ga[g_ix], gb[g_ix]) {
              straddling = true;
              if let Some(root) = bisect_guard_root(pa, pb, 0., 1., g_ix, guards.unwrap()) {
                if dedup.try_insert(root) {
                  record_crease(crease, root, g_ix);
                  fresh.push(root.x);
                  fresh.push(root.y);
                  inserted = true;
                }
              }
            }
          }
        }
      }

      if !inserted {
        if straddling {
          // Every crossing root already exists within the dedup radius — the crease is resolved
          // to that precision and the residual over-tol reading is a chord-probe artifact (e.g.
          // at X-crossing saddles), so this neither inserts nor counts as unresolved.
          continue;
        }
        // Rung 0 floor: an over-tol triangle this small is chasing something splitting can't fix
        // (unaligned crease, value jump) — leave it and report instead of exploding.
        let longest = (verts_2d[j] - verts_2d[i])
          .norm()
          .max((verts_2d[k] - verts_2d[j]).norm())
          .max((verts_2d[i] - verts_2d[k]).norm());
        if longest < env.min_split_edge {
          stats.stalled += 1;
          continue;
        }
        let c = (verts_2d[i] + verts_2d[j] + verts_2d[k]) / 3.;
        if dedup.try_insert(c) {
          fresh.push(c.x);
          fresh.push(c.y);
        } else {
          stats.stalled += 1;
        }
      }
    }

    result = Some((verts_2d, verts_3d, indices));
    if fresh.is_empty() {
      stats.converged = stats.stalled == 0;
      break;
    }
    if stats.budget_hit {
      break;
    }
    // On budget exhaustion, keep the batch prefix that still fits and run one more iteration so
    // it actually gets triangulated into the result.
    let room = REFINE_POINT_BUDGET.saturating_sub(interior.len() / 2) * 2;
    if fresh.len() > room {
      stats.budget_hit = true;
    }
    interior.extend_from_slice(&fresh[..fresh.len().min(room)]);
    if room == 0 {
      break;
    }
  }

  let (mut domain_uvs, verts, mut indices) = result.unwrap();
  let verts: Vec<Vec3> = verts.iter().map(|s| s.pos(sel)).collect();
  // Same −(φ_u × φ_v) default facing as `tessellate_2d_paths_embedded`.
  if !env.flipped {
    for tri in indices.chunks_mut(3) {
      tri.swap(0, 2);
    }
  }
  let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);

  // Crease-alignment post-pass (single round, topology-side).  The loop above is deviation-gated,
  // so a crease whose kink is *sub-tolerance* (shallow double-valleys) attracts no roots and
  // renders as a soft diamond at X-crossings.  Walk every final edge once and `split_edge` it in
  // place at its first clear guard crossing (with a midpoint sample so an edge crossing two
  // crease lines — an even sign flip the endpoint test is blind to — is still caught).  Splitting
  // never re-triangulates, so there is no CDT quality-refinement feedback: sub-edges end *on* the
  // crease (guard ≈ 0, `clear_straddle` rejects) and can't re-register, and when a crease crosses
  // two edges of one triangle the second split's spoke connects the two on-crease vertices,
  // edge-chaining the crease for the sharp-marking downstream.
  if let Some(g) = guards {
    let mut guard_at = |uv: Vec2| -> Option<Rc<Vec<f32>>> {
      guard_cache
        .entry([uv.x.to_bits(), uv.y.to_bits()])
        .or_insert_with(|| g(uv).map(Rc::new))
        .clone()
    };
    for ek in mesh.edges.keys().collect::<Vec<_>>() {
      // border-adjacent edges can be dropped + recreated by an earlier split's face rebuild
      let Some(edge) = mesh.edges.get(ek) else {
        continue;
      };
      let [va, vb] = edge.vertices;
      let (pa, pb) = (
        domain_uvs[vkey_ix(&va) as usize - 1],
        domain_uvs[vkey_ix(&vb) as usize - 1],
      );
      if (pb - pa).norm() <= DEFAULT_MIN_SEGMENT_LENGTH * 2. {
        continue;
      }
      let (Some(ga), Some(gb)) = (guard_at(pa), guard_at(pb)) else {
        continue;
      };
      // Quarter-point samples: interior crossings hide from the endpoint test both when the flip
      // count is even and when an endpoint sits on the crease itself (near-zero guard, rejected
      // by `clear_straddle`) — finer brackets shrink both blind windows.
      let (Some(g25), Some(g50), Some(g75)) = (
        g(pa + (pb - pa) * 0.25),
        g((pa + pb) * 0.5),
        g(pa + (pb - pa) * 0.75),
      ) else {
        continue;
      };
      let samples: [(f32, &[f32]); 5] = [
        (0., &ga),
        (0.25, &g25),
        (0.5, &g50),
        (0.75, &g75),
        (1., &gb),
      ];
      let n_guards = samples.iter().map(|(_, v)| v.len()).min().unwrap();
      let mut found: Option<(Vec2, usize)> = None;
      'guards: for k in 0..n_guards {
        for w in samples.windows(2) {
          let ((lo, glo), (hi, ghi)) = (w[0], w[1]);
          if clear_straddle(glo[k], ghi[k]) {
            if let Some(root) = bisect_guard_root(pa, pb, lo, hi, k, g) {
              if dedup.try_insert(root) {
                found = Some((root, k));
                break 'guards;
              }
            }
          }
        }
      }
      let Some((root, k)) = found else {
        continue;
      };
      let t = (root - pa).norm() / (pb - pa).norm();
      if !(1e-4..=1. - 1e-4).contains(&t) {
        continue;
      }
      let new_vk = mesh.split_edge(
        ek,
        EdgeSplitPos {
          pos: t,
          start_vtx_key: va,
        },
        DisplacementNormalMethod::Interpolate,
      );
      mesh.vertices[new_vk].position = probe(root)?.pos(sel);
      record_crease(crease, root, k);
      debug_assert_eq!(vkey_ix(&new_vk) as usize - 1, domain_uvs.len());
      domain_uvs.push(root);
    }
  }

  Ok((mesh, domain_uvs, stats))
}

/// One boundary loop of a two-cap solid: per ring slot, the top-cap / offset-cap vertex index
/// (each cap's own `vkey_ix - 1` space).  Both rails follow the same domain point sequence in
/// wall-canonical direction (the border-edge orientation inside the *assembled*, flipped top-cap
/// face), so slot k pairs 1:1 for wall stitching.
pub struct TessRing {
  pub top: Vec<usize>,
  pub off: Vec<usize>,
}

/// The assembled closed solid from [`tessellate_2d_paths_embedded_two_caps`], plus everything
/// `embed_path` needs to sharp-mark and author attributes onto it.  Top-cap vertices are
/// `vkey(i + 1, 1)` aligned with `top_uvs`; offset-cap vertex keys are listed in `off_vkeys`,
/// aligned with `off_uvs`.  Wall faces are the ones in neither face set.
pub struct TwoCapSolid {
  pub mesh: LinkedMesh<()>,
  pub top_uvs: Vec<Vec2>,
  pub off_uvs: Vec<Vec2>,
  pub off_vkeys: Vec<VertexKey>,
  pub top_faces: FxHashSet<FaceKey>,
  pub off_faces: FxHashSet<FaceKey>,
  pub top_crease: CreaseUvs,
  pub off_crease: CreaseUvs,
  pub rings: Vec<TessRing>,
  pub stats: RefineStats,
}

struct CapBorder {
  adj: FxHashMap<usize, Vec<usize>>,
  ix_by_uv: FxHashMap<[u32; 2], usize>,
}

fn cap_border(mesh: &LinkedMesh<()>, uvs: &[Vec2]) -> CapBorder {
  let mut adj: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
  for edge in mesh.edges.values() {
    if edge.faces.len() != 1 {
      continue;
    }
    let a = vkey_ix(&edge.vertices[0]) as usize - 1;
    let b = vkey_ix(&edge.vertices[1]) as usize - 1;
    adj.entry(a).or_default().push(b);
    adj.entry(b).or_default().push(a);
  }
  let ix_by_uv = uvs
    .iter()
    .enumerate()
    .map(|(i, uv)| ([uv.x.to_bits(), uv.y.to_bits()], i))
    .collect();
  CapBorder { adj, ix_by_uv }
}

fn uv_bits(uv: Vec2) -> [u32; 2] {
  [uv.x.to_bits(), uv.y.to_bits()]
}

/// Walks the border loop containing `dense_ring`'s vertices (all preserved as constrained CDT
/// vertices), returning its vertex indices starting at `dense_ring[0]` and oriented to the dense
/// ring's direction.  `None` when the loop structure is unexpected (e.g. loops touching at a
/// vertex, where a border vertex has more than 2 border neighbors).
fn walk_boundary_ring(border: &CapBorder, dense_ring: &[Vec2]) -> Option<Vec<usize>> {
  let start = *border.ix_by_uv.get(&uv_bits(dense_ring[0]))?;
  let mut ring = vec![start];
  let nbrs = border.adj.get(&start)?;
  if nbrs.len() != 2 {
    return None;
  }
  let (mut prev, mut cur) = (start, nbrs[0]);
  while cur != start {
    ring.push(cur);
    if ring.len() > border.adj.len() {
      return None;
    }
    let nbrs = border.adj.get(&cur)?;
    if nbrs.len() != 2 {
      return None;
    }
    let next = if nbrs[0] == prev { nbrs[1] } else { nbrs[0] };
    prev = cur;
    cur = next;
  }
  // Dense vertices appear on the loop in cyclic order, so the walk direction matches the dense
  // direction iff dense[1] shows up before dense[2].
  let pos_in_ring: FxHashMap<usize, usize> =
    ring.iter().enumerate().map(|(k, &ix)| (ix, k)).collect();
  let p1 = *pos_in_ring.get(border.ix_by_uv.get(&uv_bits(dense_ring[1]))?)?;
  let p2 = *pos_in_ring.get(border.ix_by_uv.get(&uv_bits(dense_ring[2]))?)?;
  if p1 > p2 {
    ring[1..].reverse();
  }
  Some(ring)
}

/// Slot-merge tolerance along a dense boundary segment: points from the two caps within this
/// parametric distance are treated as one shared rail slot instead of split into sliver rungs.
const RING_SNAP_T: f32 = 1e-4;

/// Splices each cap's missing boundary vertices into the other's rail (via `split_edge` on the
/// bracketing border edge, positioned on that cap's own surface) so both rails carry the same
/// domain point sequence.  Needed because the min-angle mesher (encroachment) and the crease
/// post-pass each split border edges independently per cap.
fn conform_ring_pair(
  dense_ring: &[Vec2],
  ring_a: &[usize],
  mesh_a: &mut LinkedMesh<()>,
  uvs_a: &mut Vec<Vec2>,
  sel_a: SurfSel,
  ring_b: &[usize],
  mesh_b: &mut LinkedMesh<()>,
  uvs_b: &mut Vec<Vec2>,
  sel_b: SurfSel,
  probe: Probe,
) -> Result<Option<(Vec<usize>, Vec<usize>)>, ErrorStack> {
  let m = dense_ring.len();
  let anchors = |ring: &[usize], uvs: &[Vec2]| -> Option<Vec<usize>> {
    let dense_slot: FxHashMap<[u32; 2], usize> = dense_ring
      .iter()
      .enumerate()
      .map(|(s, uv)| (uv_bits(*uv), s))
      .collect();
    let mut out = vec![usize::MAX; m];
    for (k, &ix) in ring.iter().enumerate() {
      if let Some(&s) = dense_slot.get(&uv_bits(uvs[ix])) {
        out[s] = k;
      }
    }
    (out.iter().all(|&k| k != usize::MAX) && out.windows(2).all(|w| w[0] < w[1])).then_some(out)
  };
  let (Some(anchors_a), Some(anchors_b)) = (anchors(ring_a, uvs_a), anchors(ring_b, uvs_b)) else {
    return Ok(None);
  };

  let mut out_a: Vec<usize> = Vec::with_capacity(ring_a.len().max(ring_b.len()));
  let mut out_b: Vec<usize> = Vec::with_capacity(out_a.capacity());
  let split = |mesh: &mut LinkedMesh<()>,
               uvs: &mut Vec<Vec2>,
               sel: SurfSel,
               prev: usize,
               next: usize,
               rel: f32,
               uv: Vec2|
   -> Result<usize, ErrorStack> {
    let va = vkey(prev as u32 + 1, 1);
    let vb = vkey(next as u32 + 1, 1);
    let ek = mesh
      .get_edge_key([va, vb])
      .ok_or_else(|| ErrorStack::new("two-cap ring conform: bracketing border edge not found"))?;
    let new_vk = mesh.split_edge(
      ek,
      EdgeSplitPos {
        pos: rel.clamp(1e-3, 1. - 1e-3),
        start_vtx_key: va,
      },
      DisplacementNormalMethod::Interpolate,
    );
    mesh.vertices[new_vk].position = probe(uv)?.pos(sel);
    debug_assert_eq!(vkey_ix(&new_vk) as usize - 1, uvs.len());
    uvs.push(uv);
    Ok(vkey_ix(&new_vk) as usize - 1)
  };

  for s in 0..m {
    let (seg_a, seg_b) = (dense_ring[s], dense_ring[(s + 1) % m]);
    let seg = seg_b - seg_a;
    let inv_len2 = 1. / seg.norm_squared();
    let extras = |ring: &[usize], uvs: &[Vec2], anchors: &[usize]| -> Vec<(f32, usize)> {
      let from = anchors[s] + 1;
      let to = if s + 1 < m {
        anchors[s + 1]
      } else {
        ring.len()
      };
      ring[from..to]
        .iter()
        .map(|&ix| ((uvs[ix] - seg_a).dot(&seg) * inv_len2, ix))
        .collect()
    };
    let ea = extras(ring_a, uvs_a, &anchors_a);
    let eb = extras(ring_b, uvs_b, &anchors_b);

    // Merge the two rails' extra points into shared slots by parametric position.
    let mut slots: Vec<(Option<usize>, Option<usize>, f32)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < ea.len() || j < eb.len() {
      match (ea.get(i).copied(), eb.get(j).copied()) {
        (Some((ta, ixa)), Some((tb, ixb))) => {
          if uv_bits(uvs_a[ixa]) == uv_bits(uvs_b[ixb]) || (ta - tb).abs() < RING_SNAP_T {
            slots.push((Some(ixa), Some(ixb), ta));
            i += 1;
            j += 1;
          } else if ta < tb {
            slots.push((Some(ixa), None, ta));
            i += 1;
          } else {
            slots.push((None, Some(ixb), tb));
            j += 1;
          }
        }
        (Some((ta, ixa)), None) => {
          slots.push((Some(ixa), None, ta));
          i += 1;
        }
        (None, Some((tb, ixb))) => {
          slots.push((None, Some(ixb), tb));
          j += 1;
        }
        (None, None) => unreachable!(),
      }
    }

    // Next existing rail vertex after each slot (defaults to the segment-end anchor at t=1).
    let next_existing = |get: &dyn Fn(&(Option<usize>, Option<usize>, f32)) -> Option<usize>,
                         end: usize|
     -> Vec<(usize, f32)> {
      let mut out = vec![(end, 1.); slots.len()];
      let mut carry = (end, 1.);
      for si in (0..slots.len()).rev() {
        out[si] = carry;
        if let Some(ix) = get(&slots[si]) {
          carry = (ix, slots[si].2);
        }
      }
      out
    };
    let end_a = ring_a[if s + 1 < m { anchors_a[s + 1] } else { 0 }];
    let end_b = ring_b[if s + 1 < m { anchors_b[s + 1] } else { 0 }];
    let next_a = next_existing(&|slot| slot.0, end_a);
    let next_b = next_existing(&|slot| slot.1, end_b);

    out_a.push(ring_a[anchors_a[s]]);
    out_b.push(ring_b[anchors_b[s]]);
    let mut prev_a = (ring_a[anchors_a[s]], 0f32);
    let mut prev_b = (ring_b[anchors_b[s]], 0f32);
    for (si, &(a, b, t)) in slots.iter().enumerate() {
      let a_ix = match a {
        Some(ix) => ix,
        None => {
          let (nix, nt) = next_a[si];
          let uv = uvs_b[b.unwrap()];
          split(
            mesh_a,
            uvs_a,
            sel_a,
            prev_a.0,
            nix,
            (t - prev_a.1) / (nt - prev_a.1),
            uv,
          )?
        }
      };
      let b_ix = match b {
        Some(ix) => ix,
        None => {
          let (nix, nt) = next_b[si];
          let uv = uvs_a[a.unwrap()];
          split(
            mesh_b,
            uvs_b,
            sel_b,
            prev_b.0,
            nix,
            (t - prev_b.1) / (nt - prev_b.1),
            uv,
          )?
        }
      };
      out_a.push(a_ix);
      out_b.push(b_ix);
      prev_a = (a_ix, t);
      prev_b = (b_ix, t);
    }
  }

  Ok(Some((out_a, out_b)))
}

/// Whether `ring`'s direction matches the border-edge orientation inside its (pre-reversal) cap
/// face.  The wall-canonical direction is the *opposite*: `extrude_with_offsets` reverses the
/// original faces before deriving the border direction, so its walls follow the flipped cycle.
fn ring_matches_face_winding(mesh: &LinkedMesh<()>, ring: &[usize]) -> Option<bool> {
  let va = vkey(ring[0] as u32 + 1, 1);
  let vb = vkey(ring[1] as u32 + 1, 1);
  let ek = mesh.get_edge_key([va, vb])?;
  let &fk = mesh.edges[ek].faces.first()?;
  let vs = mesh.faces[fk].vertices;
  Some((0..3).any(|i| vs[i] == va && vs[(i + 1) % 3] == vb))
}

/// §12.2b per-cap independent interior tessellation: refines the top cap for φ's curvature only
/// and the offset cap for Ψ's, sharing just the densified boundary (which still tracks the worse
/// of both surfaces — the walls are ruled between the rails), then assembles the closed solid
/// directly — both caps plus ruled wall strips — instead of mirroring one CDT via extrusion.  A
/// spatially-varying thickness no longer duplicates its crease/curvature Steiner points onto the
/// (often much flatter) top cap.
///
/// `guards_top` is the embed-sourced guard subset: thickness creases don't kink φ, so the top
/// pass must not chase them (nor edge-split them in the crease post-pass).  The boundary pass
/// keeps the full set — thickness creases crossing the boundary kink the walls.
///
/// Returns `Ok(None)` when a topology precondition fails (boundary loops touching at a vertex);
/// the caller falls back to the shared-CDT path.
#[allow(clippy::too_many_arguments)]
pub fn tessellate_2d_paths_embedded_two_caps(
  paths: &[Vec<Vec2>],
  flipped: bool,
  tol: f32,
  embed: impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
  offset_surface: &dyn Fn(Vec2) -> Result<Vec3, ErrorStack>,
  guards: Option<GuardEval>,
  guards_top: Option<GuardEval>,
) -> Result<Option<TwoCapSolid>, ErrorStack> {
  if paths.is_empty() {
    return Ok(None);
  }

  let probe_cache: RefCell<FxHashMap<[u32; 2], SurfPt>> = RefCell::new(FxHashMap::default());
  let probe = |uv: Vec2| -> Result<SurfPt, ErrorStack> {
    let key = uv_bits(uv);
    if let Some(&hit) = probe_cache.borrow().get(&key) {
      return Ok(hit);
    }
    let pt = SurfPt {
      top: embed(uv)?,
      off: offset_surface(uv)?,
    };
    probe_cache.borrow_mut().insert(key, pt);
    Ok(pt)
  };

  let mut boundary_crease = CreaseUvs::default();
  let dense = paths
    .iter()
    .map(|path| {
      let with_roots = guards
        .and_then(|g| insert_boundary_guard_roots(path, g, &mut boundary_crease))
        .unwrap_or_else(|| path.clone());
      densify_loop_under_embed(
        &with_roots,
        &probe,
        guards,
        tol,
        SurfSel::Both,
        &mut boundary_crease,
      )
    })
    .collect::<Result<Vec<_>, _>>()?;
  let (coords, subpath_lengths) = flatten_paths(&dense)?;
  let (min_split_edge, dedup_radius) = domain_scales(&coords);
  let env = RefineEnv {
    probe: &probe,
    coords: &coords,
    subpath_lengths: &subpath_lengths,
    flipped,
    tol,
    min_split_edge,
    dedup_radius,
  };

  let mut top_crease = boundary_crease.clone();
  let mut off_crease = boundary_crease;
  let (mut mesh_top, mut top_uvs, stats_top) =
    refine_cap(&env, SurfSel::Top, guards_top, &mut top_crease)?;
  let (mut mesh_off, mut off_uvs, stats_off) =
    refine_cap(&env, SurfSel::Off, guards, &mut off_crease)?;
  let stats = RefineStats {
    stalled: stats_top.stalled + stats_off.stalled,
    budget_hit: stats_top.budget_hit || stats_off.budget_hit,
    converged: stats_top.converged && stats_off.converged,
  };

  let walked: Option<Vec<(Vec<usize>, Vec<usize>)>> = {
    let border_top = cap_border(&mesh_top, &top_uvs);
    let border_off = cap_border(&mesh_off, &off_uvs);
    dense
      .iter()
      .map(|dr| {
        Some((
          walk_boundary_ring(&border_top, dr)?,
          walk_boundary_ring(&border_off, dr)?,
        ))
      })
      .collect()
  };
  let Some(walked) = walked else {
    return Ok(None);
  };

  let mut rings: Vec<TessRing> = Vec::with_capacity(dense.len());
  for (dense_ring, (ring_top, ring_off)) in dense.iter().zip(&walked) {
    let Some((top, off)) = conform_ring_pair(
      dense_ring,
      ring_top,
      &mut mesh_top,
      &mut top_uvs,
      SurfSel::Top,
      ring_off,
      &mut mesh_off,
      &mut off_uvs,
      SurfSel::Off,
      &probe,
    )?
    else {
      return Ok(None);
    };
    rings.push(TessRing { top, off });
  }

  // Wall-canonical direction is the border edge's orientation in the *assembled* (flipped)
  // top-cap face — `extrude_with_offsets` derives it after reversing the originals — so a ring
  // aligned with the pre-reversal face here must be reversed, else the walls wind inward.
  for ring in &mut rings {
    match ring_matches_face_winding(&mesh_top, &ring.top) {
      Some(true) => {
        ring.top.reverse();
        ring.off.reverse();
      }
      Some(false) => {}
      None => return Ok(None),
    }
  }

  // Assemble, mirroring `extrude_along_normals`' conventions: the top cap's faces flip to face
  // outward, the offset cap keeps the tessellator winding, and walls follow the post-flip
  // border-edge direction.
  let top_faces: FxHashSet<FaceKey> = mesh_top.faces.keys().collect();
  let mut mesh = mesh_top;
  for &fk in &top_faces {
    mesh.faces[fk].vertices.swap(0, 2);
  }
  let off_vkeys: Vec<VertexKey> = (0..off_uvs.len())
    .map(|j| {
      let pos = mesh_off.vertices[vkey(j as u32 + 1, 1)].position;
      mesh.vertices.insert(Vertex::new(pos))
    })
    .collect();
  let mut off_faces = FxHashSet::default();
  for face in mesh_off.faces.values() {
    let vs = face.vertices.map(|vk| off_vkeys[vkey_ix(&vk) as usize - 1]);
    off_faces.insert(mesh.add_face::<false>(vs, ()));
  }
  for ring in &rings {
    let m = ring.top.len();
    for k in 0..m {
      let v0 = vkey(ring.top[k] as u32 + 1, 1);
      let v1 = vkey(ring.top[(k + 1) % m] as u32 + 1, 1);
      let nv0 = off_vkeys[ring.off[k]];
      let nv1 = off_vkeys[ring.off[(k + 1) % m]];
      mesh.add_face::<false>([nv1, v1, v0], ());
      mesh.add_face::<false>([nv0, nv1, v0], ());
    }
  }

  Ok(Some(TwoCapSolid {
    mesh,
    top_uvs,
    off_uvs,
    off_vkeys,
    top_faces,
    off_faces,
    top_crease,
    off_crease,
    rings,
    stats,
  }))
}

/// Which surface-normal source `embed_path` uses — always for the thickening offset direction, and
/// additionally for the authored cap shading normals when `split_seams` is set.  `Auto` picks the
/// best available — exact symbolic autodiff of the embedding when it is differentiable, else finite
/// differences.  The explicit variants force one method (validation/debugging); `Mesh` reverts to
/// topological face-weighted normals.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NormalMode {
  Auto,
  Autodiff,
  FiniteDiff,
  Mesh,
}

impl NormalMode {
  pub fn parse(s: &str) -> Option<Self> {
    Some(match s {
      "auto" => Self::Auto,
      "autodiff" | "analytic" => Self::Autodiff,
      "finite_diff" | "finite" | "numeric" => Self::FiniteDiff,
      "mesh" | "topological" => Self::Mesh,
      _ => return None,
    })
  }
}

/// A finite-difference tangent frame of the embedding φ at a domain point: position, the two
/// partial derivatives, and the unit surface normal.  This is the fallback for a T2 analytic frame
/// and the first-order basis for metric-tensor (`JᵀJ`) sizing; see design §10.
#[derive(Clone, Copy, Debug)]
pub struct EmbedFrame {
  pub pos: Vec3,
  pub du: Vec3,
  pub dv: Vec3,
  pub normal: Vec3,
}

/// Estimates φ's tangent frame at `uv` by central differences with domain step `h`.  Normal is
/// `du × dv` (zero where the surface is locally degenerate).
pub fn estimate_embed_frame(
  uv: Vec2,
  h: f32,
  embed: &impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
) -> Result<EmbedFrame, ErrorStack> {
  let pos = embed(uv)?;
  let du = (embed(uv + Vec2::new(h, 0.))? - embed(uv - Vec2::new(h, 0.))?) / (2. * h);
  let dv = (embed(uv + Vec2::new(0., h))? - embed(uv - Vec2::new(0., h))?) / (2. * h);
  let cross = du.cross(&dv);
  let normal = if cross.norm() > 1e-12 {
    cross.normalize()
  } else {
    Vec3::zeros()
  };
  Ok(EmbedFrame {
    pos,
    du,
    dv,
    normal,
  })
}

/// Analytic embedding frame source: the exact partials φ_u, φ_v of an embedding closure obtained by
/// forward-mode autodiff, evaluated per domain sample.  The two derivative closures are built once
/// (via [`autodiff::build_directional_derivative`]) and reused across samples — the top rung
/// of the T2 frame-source ladder (`user frame > autodiff > finite-diff`).
pub struct AutodiffEmbedFrame {
  embed: Rc<Callable>,
  du: Rc<Callable>,
  dv: Rc<Callable>,
}

impl AutodiffEmbedFrame {
  /// Build the analytic partials of `embed` (a `vec2 -> vec3` closure).  Returns `None` when
  /// `embed` is not a plain closure or autodiff bails on some construct in its body — the caller
  /// then falls back to [`estimate_embed_frame`].
  pub fn try_build(ctx: &EvalCtx, embed: &Rc<Callable>) -> Option<AutodiffEmbedFrame> {
    let Callable::Closure(closure) = &**embed else {
      return None;
    };
    let du =
      autodiff::build_directional_derivative(ctx, closure, &Value::Vec2(Vec2::new(1., 0.))).ok()?;
    let dv =
      autodiff::build_directional_derivative(ctx, closure, &Value::Vec2(Vec2::new(0., 1.))).ok()?;
    Some(AutodiffEmbedFrame {
      embed: Rc::clone(embed),
      du: Rc::new(Callable::Closure(du)),
      dv: Rc::new(Callable::Closure(dv)),
    })
  }

  fn eval_vec3(ctx: &EvalCtx, f: &Rc<Callable>, uv: Vec2, role: &str) -> Result<Vec3, ErrorStack> {
    let out = ctx.invoke_callable(f, &[Value::Vec2(uv)], crate::EMPTY_KWARGS)?;
    out.as_vec3().copied().ok_or_else(|| {
      ErrorStack::new(format!(
        "autodiff embed frame: expected Vec3 from {role}, found: {out:?}"
      ))
    })
  }

  /// The analytic frame at `uv`: position from φ, partials from φ_u/φ_v, normal `φ_u × φ_v`.
  pub fn frame(&self, ctx: &EvalCtx, uv: Vec2) -> Result<EmbedFrame, ErrorStack> {
    let pos = Self::eval_vec3(ctx, &self.embed, uv, "embed")?;
    let du = Self::eval_vec3(ctx, &self.du, uv, "d(embed)/du")?;
    let dv = Self::eval_vec3(ctx, &self.dv, uv, "d(embed)/dv")?;
    let cross = du.cross(&dv);
    let normal = if cross.norm() > 1e-12 {
      cross.normalize()
    } else {
      Vec3::zeros()
    };
    Ok(EmbedFrame {
      pos,
      du,
      dv,
      normal,
    })
  }
}

/// Tessellates a pre-built lyon `Path` into a flat mesh in the given coordinate plane.
///
/// Output vertices are deduplicated by exact float position before building the `LinkedMesh`,
/// guarding against lyon emitting two vertices at the same coordinate for touching subpaths
/// or self-intersections.
pub fn tessellate_lyon_path(
  lyon_path: &lyon_tessellation::path::Path,
  fill_rule: lyon_tessellation::FillRule,
  flipped: bool,
  plane: TessPlane,
) -> Result<LinkedMesh<()>, ErrorStack> {
  use lyon_tessellation::{
    geom::Point, geometry_builder::Positions, BuffersBuilder, FillOptions, FillTessellator,
    VertexBuffers,
  };

  let mut buffers: VertexBuffers<Point<f32>, u32> = VertexBuffers::new();
  {
    let mut vertex_builder = BuffersBuilder::new(&mut buffers, Positions);
    FillTessellator::new()
      .tessellate_path(
        lyon_path,
        &FillOptions::default().with_fill_rule(fill_rule),
        &mut vertex_builder,
      )
      .map_err(|e| ErrorStack::new(format!("Lyon tessellation error: {e:?}")))?;
  }

  if buffers.indices.is_empty() {
    return Err(ErrorStack::new("Lyon tessellation produced no triangles"));
  }

  let mut deduped_verts: Vec<Vec3> = Vec::with_capacity(buffers.vertices.len());
  let mut vert_remap: Vec<u32> = Vec::with_capacity(buffers.vertices.len());
  let mut pos_to_idx: FxHashMap<(u32, u32), u32> = FxHashMap::default();

  for p in &buffers.vertices {
    let key = (p.x.to_bits(), p.y.to_bits());
    let idx = *pos_to_idx.entry(key).or_insert_with(|| {
      let idx = deduped_verts.len() as u32;
      deduped_verts.push(plane.embed(p.x, p.y));
      idx
    });
    vert_remap.push(idx);
  }

  let mut indices: Vec<u32> = buffers
    .indices
    .iter()
    .map(|&i| vert_remap[i as usize])
    .collect();

  // Lyon's param-space winding is opposite CGAL's, so the base swap is inverted here to reach the
  // same default face (+remaining axis); `flipped` opts into the other side.
  if plane.ccw_faces_positive() ^ flipped {
    for tri in indices.chunks_mut(3) {
      tri.swap(0, 2);
    }
  }

  Ok(LinkedMesh::from_indexed_vertices(
    &deduped_verts,
    &indices,
    None,
    None,
  ))
}

/// Tessellates one or more closed 2D paths (as `Vec<Vec2>` polylines) into a flat mesh in the
/// given coordinate plane.
///
/// Multiple paths are treated as subpaths under the given fill rule. For `PathSampler` callables
/// (e.g. from `trace_path`), prefer `tessellate_lyon_path` with a path built via
/// `to_lyon_path_for_tessellation()` which passes curve geometry (beziers, arcs) directly to
/// lyon rather than pre-discretizing.
pub fn tessellate_2d_paths_with_lyon(
  paths: &[Vec<Vec2>],
  fill_rule: lyon_tessellation::FillRule,
  flipped: bool,
  plane: TessPlane,
) -> Result<LinkedMesh<()>, ErrorStack> {
  use lyon_tessellation::{geom::Point, path::Path};

  if paths.is_empty() {
    return Ok(LinkedMesh::default());
  }

  let mut builder = Path::builder();
  for path in paths {
    if path.len() < 3 {
      return Err(ErrorStack::new(format!(
        "Cannot tessellate path with fewer than 3 points, found: {}",
        path.len()
      )));
    }
    builder.begin(Point::new(path[0].x, path[0].y));
    for pt in &path[1..] {
      builder.line_to(Point::new(pt.x, pt.y));
    }
    builder.end(true);
  }

  tessellate_lyon_path(&builder.build(), fill_rule, flipped, plane)
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod native_tests {
  use super::*;

  /// Native stand-in for CGAL's constrained-Delaunay triangulation of a single loop, so the
  /// otherwise-wasm-only embed_path refinement path is exercisable under `cargo test`.  Mirrors the
  /// CGAL semantics: quality-refines iff `options.refine()`, so the "no slivers" regression test
  /// still fails if the min-angle floor is ever dropped.  The mapping is unused by the refine
  /// caller.
  pub(super) fn spade_refined_triangulation(
    vertices: &[f32],
    options: CgalCdtOptions,
    interior_points: &[f32],
  ) -> Result<(Vec<f32>, Vec<u32>, Vec<i32>), ErrorStack> {
    use spade::{
      AngleLimit, ConstrainedDelaunayTriangulation, Point2, RefinementParameters, Triangulation,
    };

    let boundary: Vec<Vec2> = vertices
      .chunks_exact(2)
      .map(|c| Vec2::new(c[0], c[1]))
      .collect();
    let mut cdt = ConstrainedDelaunayTriangulation::<Point2<f64>>::new();
    cdt
      .add_constraint_edges(
        boundary.iter().map(|p| Point2::new(p.x as f64, p.y as f64)),
        true,
      )
      .map_err(|e| ErrorStack::new(format!("spade constraint insertion failed: {e:?}")))?;
    for c in interior_points.chunks_exact(2) {
      cdt
        .insert(Point2::new(c[0] as f64, c[1] as f64))
        .map_err(|e| ErrorStack::new(format!("spade interior insertion failed: {e:?}")))?;
    }
    if let Some(sq_sine) = options.min_angle_squared_sine.filter(|_| options.refine()) {
      cdt.refine(
        RefinementParameters::<f64>::new()
          .with_angle_limit(AngleLimit::from_deg(
            (sq_sine.sqrt().asin().to_degrees()) as f64,
          ))
          .exclude_outer_faces(true),
      );
    }

    let mut out_xy = vec![0f32; cdt.num_vertices() * 2];
    for v in cdt.vertices() {
      let i = v.fix().index();
      let p = v.position();
      out_xy[i * 2] = p.x as f32;
      out_xy[i * 2 + 1] = p.y as f32;
    }

    let mut indices = Vec::new();
    for f in cdt.inner_faces() {
      let vs = f.vertices();
      let [a, b, c] = [vs[0].position(), vs[1].position(), vs[2].position()];
      let centroid = Vec2::new(
        ((a.x + b.x + c.x) / 3.) as f32,
        ((a.y + b.y + c.y) / 3.) as f32,
      );
      if crate::mesh_ops::rail_sweep::point_in_polygon2d(centroid, &boundary) {
        for v in vs {
          indices.push(v.fix().index() as u32);
        }
      }
    }
    Ok((out_xy, indices, Vec::new()))
  }

  fn xy_plane_frame() -> PlaneFrame {
    PlaneFrame {
      center: Vec3::new(0., 0., 0.),
      u_axis: Vec3::new(1., 0., 0.),
      v_axis: Vec3::new(0., 1., 0.),
    }
  }

  // Finite-difference φ derivatives must match known analytic surfaces: a plane has axis-aligned
  // tangents; a cylinder has an axial ∂u and a radial normal.
  #[test]
  fn embed_derivatives_match_analytic_surfaces() {
    let plane = |p: Vec2| -> Result<Vec3, ErrorStack> { Ok(Vec3::new(p.x, 0., p.y)) };
    let f = estimate_embed_frame(Vec2::new(0.3, -0.7), 0.01, &plane).unwrap();
    assert!((f.du - Vec3::new(1., 0., 0.)).norm() < 1e-3);
    assert!((f.dv - Vec3::new(0., 0., 1.)).norm() < 1e-3);
    assert!(f.normal.y.abs() > 0.999); // ±Y

    let r = 2.0f32;
    let cyl = |p: Vec2| -> Result<Vec3, ErrorStack> {
      Ok(Vec3::new(r * (p.x / r).cos(), p.y, r * (p.x / r).sin()))
    };
    // At u=0: pos=(R,0,0), ∂u=(0,0,1), ∂v=(0,1,0), normal radial (±X).
    let f = estimate_embed_frame(Vec2::new(0., 1.5), 0.01, &cyl).unwrap();
    assert!((f.du - Vec3::new(0., 0., 1.)).norm() < 1e-3);
    assert!(f.normal.x.abs() > 0.999);
  }

  // T2 frame-source ladder: the analytic (autodiff) frame of a curved embedding must agree with the
  // finite-difference frame, and building it must fail gracefully (→ finite-diff fallback) when the
  // embedding uses a builtin autodiff doesn't know.
  #[test]
  fn autodiff_embed_frame_matches_finite_diff() {
    let src = r#"
bump = |p: vec2|: vec3 {
  r2 = p.x*p.x + p.y*p.y
  vec3(p.x, exp(-r2), p.y)
}
opaque = |p: vec2|: vec3 { vec3(p.x, fbm(vec3(p.x, p.y, 0)), p.y) }
"#;
    let ctx = crate::parse_and_eval_program(src).unwrap();
    let Value::Callable(bump) = ctx.get_global("bump").unwrap() else {
      panic!("bump not callable")
    };

    let analytic = AutodiffEmbedFrame::try_build(&ctx, &bump).expect("autodiff should handle bump");
    let embed = |p: Vec2| -> Result<Vec3, ErrorStack> {
      let r2 = p.x * p.x + p.y * p.y;
      Ok(Vec3::new(p.x, (-r2).exp(), p.y))
    };
    for &(x, y) in &[(0.3, -0.6), (1.1, 0.4), (-0.7, 0.2)] {
      let uv = Vec2::new(x, y);
      let a = analytic.frame(&ctx, uv).unwrap();
      let fd = estimate_embed_frame(uv, 1e-3, &embed).unwrap();
      assert!(
        (a.du - fd.du).norm() < 1e-2,
        "du @ {uv:?}: {:?} vs {:?}",
        a.du,
        fd.du
      );
      assert!((a.dv - fd.dv).norm() < 1e-2, "dv @ {uv:?}");
      assert!((a.normal - fd.normal).norm() < 1e-2, "normal @ {uv:?}");
    }

    // Graceful fallback: `fbm` has no derivative rule, so `try_build` returns None.
    let Value::Callable(opaque) = ctx.get_global("opaque").unwrap() else {
      panic!("opaque not callable")
    };
    assert!(
      AutodiffEmbedFrame::try_build(&ctx, &opaque).is_none(),
      "embedding with an unregistered builtin must fall back"
    );
  }

  // Boundary densification (the φ-aware part of T1) must (a) honor the deviation tolerance on every
  // emitted segment and (b) only densify edges that actually bend under the embedding — a straight
  // domain edge that stays straight under φ should not gain points.
  #[test]
  fn densify_loop_tracks_embedding_within_tol() {
    // Square whose x-edges bend under a sine embedding; whose y-edges (const x) stay straight.
    let loop_2d = [
      Vec2::new(0., 0.),
      Vec2::new(4., 0.),
      Vec2::new(4., 1.),
      Vec2::new(0., 1.),
    ];
    let embed =
      |p: Vec2| -> Result<Vec3, ErrorStack> { Ok(Vec3::new(p.x, (p.x * 2.0).sin() * 0.6, p.y)) };
    let probe = |p: Vec2| embed(p).map(|e| SurfPt { top: e, off: e });
    let tol = 0.01;
    let dense = densify_loop_under_embed(
      &loop_2d,
      &probe,
      None,
      tol,
      SurfSel::Both,
      &mut CreaseUvs::default(),
    )
    .unwrap();

    assert!(
      dense.len() > loop_2d.len(),
      "bending edges should add points; got {}",
      dense.len()
    );

    // Every consecutive segment's embedded midpoint must sit within tol of the embedded chord.
    for k in 0..dense.len() {
      let a = dense[k];
      let b = dense[(k + 1) % dense.len()];
      let mid = embed((a + b) * 0.5).unwrap();
      let chord = (embed(a).unwrap() + embed(b).unwrap()) * 0.5;
      assert!(
        (mid - chord).norm() <= tol + 1e-6,
        "segment {k} exceeds tol"
      );
    }

    // The x=0 edge (straight under φ) should contribute no interior points: no densified vertex may
    // have x≈0 except the two original corners.
    let interior_on_x0 = dense
      .iter()
      .filter(|p| p.x.abs() < 1e-4 && p.y > 1e-4 && p.y < 1.0 - 1e-4)
      .count();
    assert_eq!(interior_on_x0, 0, "straight edge must not densify");
  }

  // §12 rung 1, 1D case: guard roots become mandatory boundary vertices at their exact zero
  // crossings on every edge the crease intersects.
  #[test]
  fn boundary_guard_roots_inserted_at_zero_crossings() {
    let loop_2d = [
      Vec2::new(0., 0.),
      Vec2::new(4., 0.),
      Vec2::new(4., 1.),
      Vec2::new(0., 1.),
    ];
    let guards = |p: Vec2| Some(vec![(p.x * 2.).cos()]);
    let mut crease = CreaseUvs::default();
    let out = insert_boundary_guard_roots(&loop_2d, &guards, &mut crease).unwrap();
    assert_eq!(
      crease.len(),
      6,
      "every inserted root should be recorded as a crease point"
    );
    // cos(2x) = 0 at x = π/4, 3π/4, 5π/4, 7π/4 → 0.785, 2.356, 3.927 within [0, 4], on both
    // x-parallel edges; the x=0/x=4 edges gain nothing.
    for target in [0.785398f32, 2.356194, 3.926991] {
      for y in [0f32, 1.] {
        assert!(
          out
            .iter()
            .any(|p| (p.y - y).abs() < 1e-6 && (p.x - target).abs() < 1e-3),
          "missing root near x={target} on the y={y} edge"
        );
      }
    }
    assert_eq!(out.len(), loop_2d.len() + 6);
  }

  // §11.2 boundary broadening: a flat embed contributes zero top-cap deviation, so any boundary
  // densification must come from the offset surface (here a sine thickness field along x).  The
  // constant-thickness y-edges must stay untouched.
  #[test]
  fn densify_loop_tracks_offset_surface() {
    let loop_2d = [
      Vec2::new(0., 0.),
      Vec2::new(4., 0.),
      Vec2::new(4., 1.),
      Vec2::new(0., 1.),
    ];
    let probe = |p: Vec2| -> Result<SurfPt, ErrorStack> {
      let top = Vec3::new(p.x, 0., p.y);
      Ok(SurfPt {
        top,
        off: top + Vec3::new(0., 0.2 + (p.x * 2.0).sin() * 0.6, 0.),
      })
    };
    let dense = densify_loop_under_embed(
      &loop_2d,
      &probe,
      None,
      0.01,
      SurfSel::Both,
      &mut CreaseUvs::default(),
    )
    .unwrap();
    assert!(
      dense.len() > loop_2d.len(),
      "offset-bending edges should add points"
    );
    let interior_on_x0 = dense
      .iter()
      .filter(|p| p.x.abs() < 1e-4 && p.y > 1e-4 && p.y < 1.0 - 1e-4)
      .count();
    assert_eq!(
      interior_on_x0, 0,
      "edge straight on both surfaces must not densify"
    );
  }

  /// For a single (hole-free) loop, the with-holes cap must remap to the exact same shared-vertex
  /// triangle indices as the single-polygon cap — validating the vertex-mapping inversion and
  /// winding handling on the native (fan-fill) backend.
  #[test]
  fn ring_cap_with_holes_single_loop_matches_frame() {
    let square = [
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(1., 1., 0.),
      Vec3::new(0., 1., 0.),
    ];
    let frame = xy_plane_frame();
    for reverse in [false, true] {
      let via_frame = tessellate_ring_cap_with_frame(&square, 10, reverse, &frame).unwrap();
      let via_holes = tessellate_ring_cap_with_holes(&[&square], &[10], reverse, &frame).unwrap();
      assert_eq!(via_frame, via_holes, "reverse={reverse}");
    }
  }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod tests {
  use super::*;

  fn xy_plane_frame() -> PlaneFrame {
    PlaneFrame {
      center: Vec3::new(0., 0., 0.),
      u_axis: Vec3::new(1., 0., 0.),
      v_axis: Vec3::new(0., 1., 0.),
    }
  }

  #[test]
  fn test_tessellate_simple_triangle() {
    let ring = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(0.5, 1., 0.),
    ];
    let indices = tessellate_ring_cap_with_frame(&ring, 0, false, &xy_plane_frame()).unwrap();
    assert_eq!(indices.len(), 3);
  }

  #[test]
  fn test_tessellate_square() {
    let ring = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(1., 1., 0.),
      Vec3::new(0., 1., 0.),
    ];
    let indices = tessellate_ring_cap_with_frame(&ring, 0, false, &xy_plane_frame()).unwrap();
    // A square should be tessellated into 2 triangles (6 indices)
    assert_eq!(indices.len(), 6);
  }

  #[test]
  fn test_tessellate_with_offset() {
    let ring = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(0.5, 1., 0.),
    ];
    let indices = tessellate_ring_cap_with_frame(&ring, 10, false, &xy_plane_frame()).unwrap();
    assert!(indices.iter().all(|&idx| idx >= 10));
  }

  #[test]
  fn test_tessellate_reverse_winding() {
    let ring = vec![
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(0.5, 1., 0.),
    ];
    let frame = xy_plane_frame();
    let normal = tessellate_ring_cap_with_frame(&ring, 0, false, &frame).unwrap();
    let reversed = tessellate_ring_cap_with_frame(&ring, 0, true, &frame).unwrap();

    // Reversed winding should have indices in opposite order within each triangle
    assert_eq!(normal.len(), reversed.len());
    for i in (0..normal.len()).step_by(3) {
      assert_eq!(normal[i], reversed[i + 2]);
      assert_eq!(normal[i + 1], reversed[i + 1]);
      assert_eq!(normal[i + 2], reversed[i]);
    }
  }
}
