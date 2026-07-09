use mesh::{linked_mesh::Vec3, LinkedMesh};

use super::adaptive_sampler::DEFAULT_MIN_SEGMENT_LENGTH;
use crate::{ErrorStack, Vec2};

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

  // Match the single-polygon cap's CGAL-winding correction, then honor `reverse_winding`.
  for tri in indices.chunks_mut(3) {
    tri.swap(0, 2);
  }
  if reverse_winding {
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
    TessPlane { u_axis: 0, v_axis: 2 }
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
  tessellate_2d_paths_multi(paths, flipped, TessPlane::default(), CgalCdtOptions::default())
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
  // interior-point insertion live in the CGAL wasm.
  if subpath_lengths.len() != 1 || options.refine() || !interior_points.is_empty() {
    return Err(ErrorStack::new(
      "Multi-subpath / refining / interior-point CGAL triangulation is only available in wasm builds",
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
  if paths.is_empty() {
    return Ok(LinkedMesh::default());
  }

  let mut coords: Vec<f32> = Vec::new();
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

  let (out_vertices_xy, mut indices, _vertex_mapping) =
    run_triangulation_with_holes(&coords, &subpath_lengths, options, &[])?;
  if indices.is_empty() {
    return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
  }

  let mut verts: Vec<Vec3> = Vec::with_capacity(out_vertices_xy.len() / 2);
  for xy in out_vertices_xy.chunks_exact(2) {
    verts.push(plane.embed(xy[0], xy[1]));
  }

  // CGAL emits CCW triangles in param space; embedding into the target plane flips apparent
  // orientation for some planes.  Swap so the default face points along the plane's +remaining
  // axis (+Y for the legacy XZ default); `flipped` opts into the other side.
  if (!plane.ccw_faces_positive()) ^ flipped {
    for tri in indices.chunks_mut(3) {
      tri.swap(0, 2);
    }
  }

  Ok(LinkedMesh::from_indexed_vertices(
    &verts, &indices, None, None,
  ))
}

/// Constrained-Delaunay-triangulates `paths` (outer + holes via subpath nesting), then embeds each
/// output 2D vertex into 3D through `embed`.  This is `tessellate_2d_paths_multi` generalized from an
/// affine `TessPlane` to an arbitrary map φ: ℝ²→ℝ³.  Winding is CGAL's raw CCW-in-param order;
/// `flipped` swaps it.  The resulting single-layer cap is what `embed_path` thickens.  Also returns
/// each output vertex's 2D domain coordinate, aligned with the mesh's vertex order (`vkey(i+1, 1)`),
/// so callers can recover φ's frame per vertex.
pub fn tessellate_2d_paths_embedded(
  paths: &[Vec<Vec2>],
  flipped: bool,
  options: CgalCdtOptions,
  embed: impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
) -> Result<(LinkedMesh<()>, Vec<Vec2>), ErrorStack> {
  if paths.is_empty() {
    return Ok((LinkedMesh::default(), Vec::new()));
  }

  let mut coords: Vec<f32> = Vec::new();
  let mut subpath_lengths: Vec<u32> = Vec::with_capacity(paths.len());
  for (ix, path) in paths.iter().enumerate() {
    if path.len() < 3 {
      return Err(ErrorStack::new(format!(
        "Cannot embed subpath {ix} with fewer than 3 points, found: {}",
        path.len()
      )));
    }
    for pt in path {
      coords.push(pt.x);
      coords.push(pt.y);
    }
    subpath_lengths.push(path.len() as u32);
  }

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

  if flipped {
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

/// Recursively appends the strictly-interior subdivision points of the 2D segment `[a, b]` (with
/// pre-embedded endpoints `ea`/`eb`) so that the embedded polyline tracks φ within `tol` — the
/// deviation of the true embedded midpoint from the embedded-chord midpoint.  Standard chord-
/// flattening, but measured in embedded (3D) space so a straight domain edge that bends under φ
/// gets densified into its actual curve.
fn densify_segment_under_embed(
  a: Vec2,
  b: Vec2,
  ea: Vec3,
  eb: Vec3,
  embed: &impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
  tol: f32,
  depth: u32,
  out: &mut Vec<Vec2>,
) -> Result<(), ErrorStack> {
  if depth >= DENSIFY_MAX_DEPTH || (b - a).norm() <= DEFAULT_MIN_SEGMENT_LENGTH {
    return Ok(());
  }
  let mid = (a + b) * 0.5;
  let emid = embed(mid)?;
  if (emid - (ea + eb) * 0.5).norm() <= tol {
    return Ok(());
  }
  densify_segment_under_embed(a, mid, ea, emid, embed, tol, depth + 1, out)?;
  out.push(mid);
  densify_segment_under_embed(mid, b, emid, eb, embed, tol, depth + 1, out)?;
  Ok(())
}

/// Densifies a closed boundary loop so its embedding under `embed` deviates from the true surface
/// by at most `tol`.  Each input vertex is kept (constrained feature points ride through); interior
/// points are inserted per segment, including the wrap segment.
fn densify_loop_under_embed(
  loop_2d: &[Vec2],
  embed: &impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
  tol: f32,
) -> Result<Vec<Vec2>, ErrorStack> {
  let n = loop_2d.len();
  let mut out = Vec::with_capacity(n * 2);
  let mut embedded: Vec<Vec3> = Vec::with_capacity(n);
  for &p in loop_2d {
    embedded.push(embed(p)?);
  }
  for k in 0..n {
    let j = (k + 1) % n;
    out.push(loop_2d[k]);
    densify_segment_under_embed(
      loop_2d[k],
      loop_2d[j],
      embedded[k],
      embedded[j],
      embed,
      tol,
      0,
      &mut out,
    )?;
  }
  Ok(out)
}

/// Max deviation of the flat triangle `(a, b, c)` (embedded positions) from the true surface, probed
/// at the three edge midpoints and the centroid.
fn embedded_triangle_deviation(
  a2: Vec2,
  b2: Vec2,
  c2: Vec2,
  a3: Vec3,
  b3: Vec3,
  c3: Vec3,
  embed: &impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
) -> Result<f32, ErrorStack> {
  let mut dev = 0f32;
  for (m2, chord) in [
    ((a2 + b2) * 0.5, (a3 + b3) * 0.5),
    ((b2 + c2) * 0.5, (b3 + c3) * 0.5),
    ((c2 + a2) * 0.5, (c3 + a3) * 0.5),
    (
      (a2 + b2 + c2) / 3.0,
      (a3 + b3 + c3) / 3.0,
    ),
  ] {
    dev = dev.max((embed(m2)? - chord).norm());
  }
  Ok(dev)
}

/// Converges by ≤25 iterations for the cases exercised so far, early-terminating at convergence;
/// the cap only bites on pathological φ.  Kept generous because a too-low cap silently returns an
/// under-refined mesh (over-tolerance triangles, no warning) rather than erroring.
const REFINE_MAX_ITERS: u32 = 30;

/// Distortion-aware version of `tessellate_2d_paths_embedded`: densifies each boundary loop under φ
/// to `tol`, then refines the interior a-posteriori — constrained-Delaunay-triangulate → embed → drop
/// a Steiner point at the centroid of every triangle still deviating from φ by more than `tol` →
/// re-triangulate with the accumulated points → repeat until converged (or the iteration cap).
/// Spatially adaptive: flat regions stay coarse, curved regions densify where they exceed tolerance.
pub fn tessellate_2d_paths_embedded_refined(
  paths: &[Vec<Vec2>],
  flipped: bool,
  tol: f32,
  embed: impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
) -> Result<(LinkedMesh<()>, Vec<Vec2>), ErrorStack> {
  if paths.is_empty() {
    return Ok((LinkedMesh::default(), Vec::new()));
  }

  let mut coords: Vec<f32> = Vec::new();
  let mut subpath_lengths: Vec<u32> = Vec::with_capacity(paths.len());
  for (ix, path) in paths.iter().enumerate() {
    if path.len() < 3 {
      return Err(ErrorStack::new(format!(
        "Cannot embed subpath {ix} with fewer than 3 points, found: {}",
        path.len()
      )));
    }
    let dense = densify_loop_under_embed(path, &embed, tol)?;
    for pt in &dense {
      coords.push(pt.x);
      coords.push(pt.y);
    }
    subpath_lengths.push(dense.len() as u32);
  }

  let mut interior: Vec<f32> = Vec::new();
  let mut result: Option<(Vec<Vec2>, Vec<Vec3>, Vec<u32>)> = None;

  for _ in 0..REFINE_MAX_ITERS {
    let (out_xy, indices, _mapping) =
      run_triangulation_with_holes(&coords, &subpath_lengths, CgalCdtOptions::default(), &interior)?;
    if indices.is_empty() {
      return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
    }

    let verts_2d: Vec<Vec2> = out_xy
      .chunks_exact(2)
      .map(|xy| Vec2::new(xy[0], xy[1]))
      .collect();
    let verts_3d: Vec<Vec3> = verts_2d
      .iter()
      .map(|&p| embed(p))
      .collect::<Result<_, _>>()?;

    let mut fresh: Vec<f32> = Vec::new();
    for tri in indices.chunks_exact(3) {
      let (i, j, k) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
      let dev = embedded_triangle_deviation(
        verts_2d[i], verts_2d[j], verts_2d[k], verts_3d[i], verts_3d[j], verts_3d[k], &embed,
      )?;
      if dev > tol {
        let c = (verts_2d[i] + verts_2d[j] + verts_2d[k]) / 3.;
        fresh.push(c.x);
        fresh.push(c.y);
      }
    }

    result = Some((verts_2d, verts_3d, indices));
    if fresh.is_empty() {
      break;
    }
    interior.extend_from_slice(&fresh);
  }

  let (domain_uvs, verts, mut indices) = result.unwrap();
  if flipped {
    for tri in indices.chunks_mut(3) {
      tri.swap(0, 2);
    }
  }
  Ok((
    LinkedMesh::from_indexed_vertices(&verts, &indices, None, None),
    domain_uvs,
  ))
}

/// Which surface-normal source `embed_path` uses for both the thickening offset direction and the
/// authored cap shading normals.  `Auto` picks the best available — exact symbolic autodiff of the
/// embedding when it is differentiable, else finite differences.  The explicit variants force one
/// method (validation/debugging); `Mesh` reverts to topological face-weighted normals with no
/// authored shading normals, i.e. the classic welded output.
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

/// A finite-difference tangent frame of the embedding φ at a domain point: position, the two partial
/// derivatives, and the unit surface normal.  This is the fallback for a T2 analytic frame and the
/// first-order basis for metric-tensor (`JᵀJ`) sizing; see design §10.
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
  Ok(EmbedFrame { pos, du, dv, normal })
}

/// Analytic embedding frame source: the exact partials φ_u, φ_v of an embedding closure obtained by
/// forward-mode autodiff, evaluated per domain sample.  The two derivative closures are built once
/// (via [`crate::autodiff::build_directional_derivative`]) and reused across samples — the top rung
/// of the T2 frame-source ladder (`user frame > autodiff > finite-diff`).
pub struct AutodiffEmbedFrame {
  embed: std::rc::Rc<crate::Callable>,
  du: std::rc::Rc<crate::Callable>,
  dv: std::rc::Rc<crate::Callable>,
}

impl AutodiffEmbedFrame {
  /// Build the analytic partials of `embed` (a `vec2 -> vec3` closure).  Returns `None` when `embed`
  /// is not a plain closure or autodiff bails on some construct in its body — the caller then falls
  /// back to [`estimate_embed_frame`].
  pub fn try_build(
    ctx: &crate::EvalCtx,
    embed: &std::rc::Rc<crate::Callable>,
  ) -> Option<AutodiffEmbedFrame> {
    let crate::Callable::Closure(closure) = &**embed else {
      return None;
    };
    let du = crate::autodiff::build_directional_derivative(ctx, closure, &crate::Value::Vec2(Vec2::new(1., 0.))).ok()?;
    let dv = crate::autodiff::build_directional_derivative(ctx, closure, &crate::Value::Vec2(Vec2::new(0., 1.))).ok()?;
    Some(AutodiffEmbedFrame {
      embed: std::rc::Rc::clone(embed),
      du: std::rc::Rc::new(crate::Callable::Closure(du)),
      dv: std::rc::Rc::new(crate::Callable::Closure(dv)),
    })
  }

  fn eval_vec3(
    ctx: &crate::EvalCtx,
    f: &std::rc::Rc<crate::Callable>,
    uv: Vec2,
    role: &str,
  ) -> Result<Vec3, ErrorStack> {
    let out = ctx.invoke_callable(f, &[crate::Value::Vec2(uv)], crate::EMPTY_KWARGS)?;
    out.as_vec3().copied().ok_or_else(|| {
      ErrorStack::new(format!(
        "autodiff embed frame: expected Vec3 from {role}, found: {out:?}"
      ))
    })
  }

  /// The analytic frame at `uv`: position from φ, partials from φ_u/φ_v, normal `φ_u × φ_v`.
  pub fn frame(&self, ctx: &crate::EvalCtx, uv: Vec2) -> Result<EmbedFrame, ErrorStack> {
    let pos = Self::eval_vec3(ctx, &self.embed, uv, "embed")?;
    let du = Self::eval_vec3(ctx, &self.du, uv, "d(embed)/du")?;
    let dv = Self::eval_vec3(ctx, &self.dv, uv, "d(embed)/dv")?;
    let cross = du.cross(&dv);
    let normal = if cross.norm() > 1e-12 {
      cross.normalize()
    } else {
      Vec3::zeros()
    };
    Ok(EmbedFrame { pos, du, dv, normal })
  }
}

/// Max magnitude of principal normal curvature of the embedded surface at `uv`, from a 9-point
/// second-difference stencil and the shape operator's eigenvalues.  0 where locally flat.  Governs
/// deviation-based sizing: a facet with 2D edge `e` bows off the surface by ≈ `e²·κ/8`, so
/// `e ≈ √(8·tol/κ)` keeps it within `tol`.
pub fn estimate_embed_max_curvature(
  uv: Vec2,
  h: f32,
  embed: &impl Fn(Vec2) -> Result<Vec3, ErrorStack>,
) -> Result<f32, ErrorStack> {
  let c = embed(uv)?;
  let (pu, mu) = (embed(uv + Vec2::new(h, 0.))?, embed(uv - Vec2::new(h, 0.))?);
  let (pv, mv) = (embed(uv + Vec2::new(0., h))?, embed(uv - Vec2::new(0., h))?);
  let du = (pu - mu) / (2. * h);
  let dv = (pv - mv) / (2. * h);
  let n = du.cross(&dv);
  if n.norm() < 1e-12 {
    return Ok(0.);
  }
  let n = n.normalize();

  let h2 = h * h;
  let duu = (pu - c * 2. + mu) / h2;
  let dvv = (pv - c * 2. + mv) / h2;
  let pp = embed(uv + Vec2::new(h, h))?;
  let pm = embed(uv + Vec2::new(h, -h))?;
  let mp = embed(uv + Vec2::new(-h, h))?;
  let mm = embed(uv + Vec2::new(-h, -h))?;
  let duv = (pp - pm - mp + mm) / (4. * h2);

  // Second fundamental form (l, m, nn) and first (e, f, g); principal curvatures solve
  // det(II − κ·I) = 0 → denom·κ² − b·κ + cc = 0.
  let (l, m, nn) = (duu.dot(&n), duv.dot(&n), dvv.dot(&n));
  let (e, f, g) = (du.dot(&du), du.dot(&dv), dv.dot(&dv));
  let denom = e * g - f * f;
  if denom.abs() < 1e-20 {
    return Ok(0.);
  }
  let b = e * nn + g * l - 2. * f * m;
  let cc = l * nn - m * m;
  let disc = (b * b - 4. * denom * cc).max(0.).sqrt();
  let k1 = (b + disc) / (2. * denom);
  let k2 = (b - disc) / (2. * denom);
  Ok(k1.abs().max(k2.abs()))
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
  use std::collections::HashMap;

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
  let mut pos_to_idx: HashMap<(u32, u32), u32> = HashMap::new();

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

  fn xy_plane_frame() -> PlaneFrame {
    PlaneFrame {
      center: Vec3::new(0., 0., 0.),
      u_axis: Vec3::new(1., 0., 0.),
      v_axis: Vec3::new(0., 1., 0.),
    }
  }

  // Finite-difference φ derivatives must match known analytic surfaces: a plane is flat (κ≈0) with
  // axis-aligned tangents; a cylinder of radius R has one principal curvature 1/R and one 0, and a
  // radial normal.
  #[test]
  fn embed_derivatives_match_analytic_surfaces() {
    let plane = |p: Vec2| -> Result<Vec3, ErrorStack> { Ok(Vec3::new(p.x, 0., p.y)) };
    let f = estimate_embed_frame(Vec2::new(0.3, -0.7), 0.01, &plane).unwrap();
    assert!((f.du - Vec3::new(1., 0., 0.)).norm() < 1e-3);
    assert!((f.dv - Vec3::new(0., 0., 1.)).norm() < 1e-3);
    assert!(f.normal.y.abs() > 0.999); // ±Y
    let k = estimate_embed_max_curvature(Vec2::new(0.3, -0.7), 0.02, &plane).unwrap();
    assert!(k < 1e-3, "plane should be flat, got κ={k}");

    let r = 2.0f32;
    let cyl = |p: Vec2| -> Result<Vec3, ErrorStack> {
      Ok(Vec3::new(r * (p.x / r).cos(), p.y, r * (p.x / r).sin()))
    };
    // At u=0: pos=(R,0,0), ∂u=(0,0,1), ∂v=(0,1,0), normal radial (±X).
    let f = estimate_embed_frame(Vec2::new(0., 1.5), 0.01, &cyl).unwrap();
    assert!((f.du - Vec3::new(0., 0., 1.)).norm() < 1e-3);
    assert!(f.normal.x.abs() > 0.999);
    let k = estimate_embed_max_curvature(Vec2::new(0., 1.5), 0.02, &cyl).unwrap();
    assert!((k - 1.0 / r).abs() < 5e-3, "cylinder κ should be 1/R=0.5, got {k}");
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
opaque = |p: vec2|: vec3 { vec3(p.x, floor(p.y), p.y) }
"#;
    let ctx = crate::parse_and_eval_program(src).unwrap();
    let crate::Value::Callable(bump) = ctx.get_global("bump").unwrap() else {
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
      assert!((a.du - fd.du).norm() < 1e-2, "du @ {uv:?}: {:?} vs {:?}", a.du, fd.du);
      assert!((a.dv - fd.dv).norm() < 1e-2, "dv @ {uv:?}");
      assert!((a.normal - fd.normal).norm() < 1e-2, "normal @ {uv:?}");
    }

    // Graceful fallback: `floor` has no derivative rule, so `try_build` returns None.
    let crate::Value::Callable(opaque) = ctx.get_global("opaque").unwrap() else {
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
    let embed = |p: Vec2| -> Result<Vec3, ErrorStack> {
      Ok(Vec3::new(p.x, (p.x * 2.0).sin() * 0.6, p.y))
    };
    let tol = 0.01;
    let dense = densify_loop_under_embed(&loop_2d, &embed, tol).unwrap();

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
