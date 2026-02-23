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

pub fn tessellate_2d_paths(
  paths: &[Vec<Vec2>],
  flipped: bool,
) -> Result<LinkedMesh<()>, ErrorStack> {
  if paths.is_empty() {
    return Ok(LinkedMesh::default());
  }

  if paths.len() != 1 {
    return Err(ErrorStack::new(
      "CGAL tessellation currently supports only a single closed path (no holes)",
    ));
  }

  let mut coords = Vec::new();
  let mut verts = Vec::new();

  let points = &paths[0];
  if points.len() < 3 {
    return Err(ErrorStack::new(format!(
      "Cannot tessellate path with fewer than 3 points, found: {}",
      points.len()
    )));
  }
  for pt in points {
    coords.push(pt.x);
    coords.push(pt.y);
    verts.push(Vec3::new(pt.x, 0.0, pt.y));
  }

  let mut indices = run_triangulation(&coords, points.len())?;
  if indices.is_empty() {
    return Err(ErrorStack::new("CGAL triangulation returned no triangles"));
  }

  // CGAL triangulation winding is opposite of the previous tessellator.
  // Flip to preserve existing winding expectations, then apply optional flip.
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
