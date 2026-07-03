use mesh::{linked_mesh::Vec3, LinkedMesh};

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
    run_triangulation_with_holes(&coords, &subpath_lengths, CgalCdtOptions::default())?;
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

pub fn tessellate_2d_paths(
  paths: &[Vec<Vec2>],
  flipped: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  tessellate_2d_paths_multi(paths, flipped, CgalCdtOptions::default())
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
  ) {
    let err = cgal_get_last_error()
      .unwrap_or_else(|| "CGAL multi-subpath triangulation failed".to_owned());
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
) -> Result<(Vec<f32>, Vec<u32>, Vec<i32>), ErrorStack> {
  // Native builds only support the single-subpath, non-refining case, matching the legacy
  // fan-fill fallback that backed `tessellate_2d_paths`.  Real CDT and Delaunay refinement live
  // in the CGAL wasm.
  if subpath_lengths.len() != 1 || options.refine() {
    return Err(ErrorStack::new(
      "Multi-subpath / refining CGAL triangulation is only available in wasm builds",
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
    run_triangulation_with_holes(&coords, &subpath_lengths, options)?;
  if indices.is_empty() {
    return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
  }

  let mut verts: Vec<Vec3> = Vec::with_capacity(out_vertices_xy.len() / 2);
  for xy in out_vertices_xy.chunks_exact(2) {
    verts.push(Vec3::new(xy[0], 0.0, xy[1]));
  }

  // CGAL winding is CCW in the XY plane.  When dropped onto XZ via (x, 0, y), the apparent
  // orientation flips, so we swap to keep faces facing +Y by default — matching the old
  // tessellate_2d_paths behavior.  `flipped` then opts back into the other side.
  for tri in indices.chunks_mut(3) {
    tri.swap(0, 2);
  }
  if flipped {
    for tri in indices.chunks_mut(3) {
      tri.swap(0, 2);
    }
  }

  Ok(LinkedMesh::from_indexed_vertices(
    &verts, &indices, None, None,
  ))
}

/// Tessellates a pre-built lyon `Path` into a flat XZ-plane mesh.
///
/// Output vertices are deduplicated by exact float position before building the `LinkedMesh`,
/// guarding against lyon emitting two vertices at the same coordinate for touching subpaths
/// or self-intersections.
pub fn tessellate_lyon_path(
  lyon_path: &lyon_tessellation::path::Path,
  fill_rule: lyon_tessellation::FillRule,
  flipped: bool,
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
      deduped_verts.push(Vec3::new(p.x, 0., p.y));
      idx
    });
    vert_remap.push(idx);
  }

  let mut indices: Vec<u32> = buffers
    .indices
    .iter()
    .map(|&i| vert_remap[i as usize])
    .collect();

  if flipped {
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

/// Tessellates one or more closed 2D paths (as `Vec<Vec2>` polylines) into a flat XZ-plane mesh.
///
/// Multiple paths are treated as subpaths under the given fill rule. For `PathSampler` callables
/// (e.g. from `trace_path`), prefer `tessellate_lyon_path` with a path built via
/// `to_lyon_path_for_tessellation()` which passes curve geometry (beziers, arcs) directly to
/// lyon rather than pre-discretizing.
pub fn tessellate_2d_paths_with_lyon(
  paths: &[Vec<Vec2>],
  fill_rule: lyon_tessellation::FillRule,
  flipped: bool,
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

  tessellate_lyon_path(&builder.build(), fill_rule, flipped)
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
      let via_holes =
        tessellate_ring_cap_with_holes(&[&square], &[10], reverse, &frame).unwrap();
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
