use std::rc::Rc;
use std::str::FromStr;

#[cfg(target_arch = "wasm32")]
use mesh::linked_mesh::Mat4;
use mesh::linked_mesh::Vec3;
use mesh::LinkedMesh;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::ErrorStack;
use crate::MeshHandle;

#[cfg(target_arch = "wasm32")]
#[inline]
fn read_raw_cgal_output_mesh() -> Result<LinkedMesh<()>, ErrorStack> {
  if let Some(err) = cgal_get_last_error() {
    return Err(ErrorStack::new(err));
  }

  let out_verts = cgal_get_output_mesh_verts();
  let out_indices = cgal_get_output_mesh_indices();
  cgal_clear_output_mesh();

  Ok(LinkedMesh::from_raw_indexed(
    &out_verts,
    &out_indices,
    None,
    None,
  ))
}

#[cfg(target_arch = "wasm32")]
#[inline]
pub(crate) fn read_cgal_output_mesh(
  transform: Mat4,
  material: Option<Rc<crate::Material>>,
) -> Result<MeshHandle, ErrorStack> {
  use crate::ManifoldHandle;
  use std::cell::RefCell;

  let out_mesh = read_raw_cgal_output_mesh()?;

  Ok(MeshHandle {
    mesh: Rc::new(out_mesh),
    transform,
    manifold_handle: Rc::new(ManifoldHandle::new(0)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material,
  })
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/manifold")]
extern "C" {
  pub fn simplify(handle: usize, tolerance: f32) -> Vec<u8>;
  pub fn convex_hull(verts: &[f32]) -> Vec<u8>;
  pub fn split_by_plane(
    handle: usize,
    transform: &[f32],
    plane_normal_x: f32,
    plane_normal_y: f32,
    plane_normal_z: f32,
    plane_offset: f32,
  );
  pub fn get_split_output(split_ix: usize) -> Vec<u8>;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/geodesics")]
extern "C" {
  pub fn trace_geodesic_path(
    mesh_verts: &[f32],
    mesh_indices: &[u32],
    path: &[f32],
    full_path: bool,
    start_pos_local_space: &[f32],
    up_dir_world_space: &[f32],
  ) -> Vec<f32>;
  pub fn get_geodesic_error() -> String;
  pub fn get_geodesics_loaded() -> bool;
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_geodesics_loaded() -> bool {
  true
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasm/cgal/cgal")]
extern "C" {
  pub fn cgal_get_last_error() -> Option<String>;
  pub fn cgal_alpha_wrap_mesh(
    mesh_verts: &[f32],
    mesh_indices: &[u32],
    relative_alpha: f32,
    relative_offset: f32,
  );
  pub fn cgal_alpha_wrap_points(points: &[f32], relative_alpha: f32, relative_offset: f32);
  pub fn cgal_get_output_mesh_verts() -> Vec<f32>;
  pub fn cgal_get_output_mesh_indices() -> Vec<u32>;
  pub fn cgal_clear_output_mesh();
  pub fn cgal_catmull_smooth_mesh(mesh_verts: &[f32], mesh_indices: &[u32], iterations: u32);
  pub fn cgal_loop_smooth_mesh(mesh_verts: &[f32], mesh_indices: &[u32], iterations: u32);
  pub fn cgal_doosabin_smooth_mesh(mesh_verts: &[f32], mesh_indices: &[u32], iterations: u32);
  pub fn cgal_sqrt_smooth_mesh(mesh_verts: &[f32], mesh_indices: &[u32], iterations: u32);
  pub fn cgal_remesh_planar_patches(
    mesh_verts: &[f32],
    mesh_indices: &[u32],
    max_angle_deg: f32,
    max_offset: f32,
  );
  pub fn cgal_remesh_isotropic(
    mesh_verts: &[f32],
    mesh_indices: &[u32],
    target_edge_length: f32,
    iterations: u32,
    protect_borders: bool,
    auto_sharp_edges: bool,
    sharp_angle_threshold_degrees: f32,
  );
  pub fn cgal_remesh_delaunay(
    mesh_verts: &[f32],
    mesh_indices: &[u32],
    target_edge_length: f32,
    facet_distance: f32,
    auto_sharp_edges: bool,
    sharp_angle_threshold_degrees: f32,
  );
  pub fn cgal_get_is_loaded() -> bool;
  pub fn cgal_bevel_mesh(
    mesh_verts: &[f32],
    mesh_indices: &[u32],
    edges_to_bevel: &[u32],
    inset_amount: f32,
    subdivision_levels: u32,
  );
}

#[cfg(not(target_arch = "wasm32"))]
pub fn trace_geodesic_path(
  _mesh_verts: &[f32],
  _mesh_indices: &[u32],
  _path: &[f32],
  _full_path: bool,
  _start_pos_local_space: &[f32],
  _up_dir_world_space: &[f32],
) -> Vec<f32> {
  Vec::new()
}

#[cfg(not(target_arch = "wasm32"))]
pub fn get_geodesic_error() -> String {
  String::new()
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn verify_cgal_loaded() -> Result<(), ErrorStack> {
  if !cgal_get_is_loaded() {
    Err(ErrorStack::new_uninitialized_module("cgal"))
  } else {
    Ok(())
  }
}

#[cfg(target_arch = "wasm32")]
pub fn simplify_mesh(mesh: &MeshHandle, tolerance: f32) -> Result<MeshHandle, ErrorStack> {
  use std::cell::RefCell;

  use crate::ManifoldHandle;

  verify_cgal_loaded()?;

  if tolerance <= 0. {
    return Err(ErrorStack::new(
      "Invalid `tolerance` passed to `simplify`; must be greater than zero",
    ));
  }

  let encoded_output = simplify(mesh.get_or_create_handle()?, tolerance);

  let (manifold_handle, out_verts, out_indices) =
    crate::mesh_ops::mesh_boolean::decode_manifold_output(&encoded_output);
  let out_mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(out_verts, out_indices, None, None);
  Ok(MeshHandle {
    mesh: Rc::new(out_mesh),
    transform: mesh.transform,
    manifold_handle: Rc::new(ManifoldHandle::new(manifold_handle)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: mesh.material.clone(),
  })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn simplify_mesh(mesh: &MeshHandle, _tolerance: f32) -> Result<MeshHandle, ErrorStack> {
  Ok(mesh.clone(false, false, false))
}

#[cfg(target_arch = "wasm32")]
pub fn convex_hull_from_verts(verts: &[Vec3]) -> Result<MeshHandle, ErrorStack> {
  use std::cell::RefCell;

  use crate::ManifoldHandle;

  verify_cgal_loaded()?;

  let verts = unsafe { std::slice::from_raw_parts(verts.as_ptr() as *const f32, verts.len() * 3) };

  let encoded_output = convex_hull(verts);
  let (manifold_handle, out_verts, out_indices) =
    crate::mesh_ops::mesh_boolean::decode_manifold_output(&encoded_output);

  let out_mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(out_verts, out_indices, None, None);
  Ok(MeshHandle {
    mesh: Rc::new(out_mesh),
    transform: Mat4::identity(),
    manifold_handle: Rc::new(ManifoldHandle::new(manifold_handle)),
    aabb: RefCell::new(None),
    trimesh: RefCell::new(None),
    material: None,
  })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn convex_hull_from_verts(_verts: &[Vec3]) -> Result<MeshHandle, ErrorStack> {
  Ok(MeshHandle::new(Rc::new(LinkedMesh::new(0, 0, None))))
}

#[cfg(target_arch = "wasm32")]
pub fn split_mesh_by_plane(
  mesh: &MeshHandle,
  plane_normal: Vec3,
  plane_offset: f32,
) -> Result<(MeshHandle, MeshHandle), ErrorStack> {
  use crate::ManifoldHandle;

  verify_cgal_loaded()?;

  let handle = mesh.get_or_create_handle()?;
  split_by_plane(
    handle,
    mesh.transform.as_slice(),
    plane_normal.x,
    plane_normal.y,
    plane_normal.z,
    plane_offset,
  );

  let a = get_split_output(0);
  let (manifold_handle, out_verts, out_indices) =
    crate::mesh_ops::mesh_boolean::decode_manifold_output(&a);
  let a = MeshHandle {
    mesh: Rc::new(LinkedMesh::from_raw_indexed(
      out_verts,
      out_indices,
      None,
      None,
    )),
    transform: Mat4::identity(),
    manifold_handle: Rc::new(ManifoldHandle::new(manifold_handle)),
    aabb: mesh.aabb.clone(),
    trimesh: mesh.trimesh.clone(),
    material: mesh.material.clone(),
  };
  let b = get_split_output(1);
  let (manifold_handle, out_verts, out_indices) =
    crate::mesh_ops::mesh_boolean::decode_manifold_output(&b);
  let b = MeshHandle {
    mesh: Rc::new(LinkedMesh::from_raw_indexed(
      out_verts,
      out_indices,
      None,
      None,
    )),
    transform: Mat4::identity(),
    manifold_handle: Rc::new(ManifoldHandle::new(manifold_handle)),
    aabb: mesh.aabb.clone(),
    trimesh: mesh.trimesh.clone(),
    material: mesh.material.clone(),
  };

  Ok((a, b))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn split_mesh_by_plane(
  _mesh: &MeshHandle,
  _plane_normal: Vec3,
  _plane_offset: f32,
) -> Result<(MeshHandle, MeshHandle), ErrorStack> {
  Err(ErrorStack::new(
    "Mesh splitting by plane is not supported outside of wasm.",
  ))
}

#[cfg(target_arch = "wasm32")]
pub fn alpha_wrap_mesh(
  mesh: &MeshHandle,
  relative_alpha: f32,
  relative_offset: f32,
) -> Result<MeshHandle, ErrorStack> {
  verify_cgal_loaded()?;

  let raw_mesh = mesh.mesh.to_raw_indexed(false, false, true);

  assert_eq!(std::mem::size_of::<usize>(), std::mem::size_of::<u32>());
  let in_indices = unsafe {
    std::slice::from_raw_parts(
      raw_mesh.indices.as_ptr() as *const u32,
      raw_mesh.indices.len(),
    )
  };

  cgal_alpha_wrap_mesh(
    &raw_mesh.vertices,
    in_indices,
    relative_alpha,
    relative_offset,
  );

  read_cgal_output_mesh(mesh.transform, mesh.material.clone())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn alpha_wrap_mesh(
  _mesh: &MeshHandle,
  _relative_alpha: f32,
  _relative_offset: f32,
) -> Result<MeshHandle, ErrorStack> {
  Err(ErrorStack::new(
    "Alpha wrapping is not supported outside of wasm",
  ))
}

pub enum SmoothType {
  CatmullClark,
  Loop,
  DooSabin,
  Sqrt,
}

impl FromStr for SmoothType {
  type Err = ErrorStack;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    match s.to_lowercase().as_str() {
      "catmull" | "catmullclark" | "catmull-clark" | "catmull_clark" | "catmull clark" => {
        Ok(SmoothType::CatmullClark)
      }
      "loop" => Ok(SmoothType::Loop),
      "doosabin" | "doo-sabin" | "doo_sabin" | "doo sabin" => Ok(SmoothType::DooSabin),
      "sqrt" => Ok(SmoothType::Sqrt),
      _ => Err(ErrorStack::new(format!(
        "Invalid smooth type: {s}.  Expected one of: CatmullClark, Loop, DooSabin, Sqrt",
      ))),
    }
  }
}

#[cfg(target_arch = "wasm32")]
pub fn smooth_mesh(
  mesh: &MeshHandle,
  smooth_type: SmoothType,
  iterations: u32,
) -> Result<MeshHandle, ErrorStack> {
  verify_cgal_loaded()?;

  let raw_mesh = mesh.mesh.to_raw_indexed(false, false, true);

  assert_eq!(std::mem::size_of::<usize>(), std::mem::size_of::<u32>());
  let in_indices = unsafe {
    std::slice::from_raw_parts(
      raw_mesh.indices.as_ptr() as *const u32,
      raw_mesh.indices.len(),
    )
  };

  match smooth_type {
    SmoothType::CatmullClark => {
      cgal_catmull_smooth_mesh(&raw_mesh.vertices, in_indices, iterations);
    }
    SmoothType::Loop => {
      cgal_loop_smooth_mesh(&raw_mesh.vertices, in_indices, iterations);
    }
    SmoothType::DooSabin => {
      cgal_doosabin_smooth_mesh(&raw_mesh.vertices, in_indices, iterations);
    }
    SmoothType::Sqrt => {
      cgal_sqrt_smooth_mesh(&raw_mesh.vertices, in_indices, iterations);
    }
  }

  read_cgal_output_mesh(mesh.transform, mesh.material.clone())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn smooth_mesh(
  _mesh: &MeshHandle,
  _smooth_type: SmoothType,
  _iterations: u32,
) -> Result<MeshHandle, ErrorStack> {
  Err(ErrorStack::new(
    "Mesh smoothing is not supported outside of wasm",
  ))
}

#[cfg(target_arch = "wasm32")]
pub fn alpha_wrap_points(
  points: &[Vec3],
  relative_alpha: f32,
  relative_offset: f32,
) -> Result<MeshHandle, ErrorStack> {
  verify_cgal_loaded()?;

  let points =
    unsafe { std::slice::from_raw_parts(points.as_ptr() as *const f32, points.len() * 3) };
  cgal_alpha_wrap_points(points, relative_alpha, relative_offset);

  read_cgal_output_mesh(Mat4::identity(), None)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn alpha_wrap_points(
  _points: &[Vec3],
  _relative_alpha: f32,
  _relative_offset: f32,
) -> Result<MeshHandle, ErrorStack> {
  Err(ErrorStack::new(
    "Alpha wrapping is not supported outside of wasm",
  ))
}

#[cfg(target_arch = "wasm32")]
pub fn remesh_planar_patches(
  mesh: &MeshHandle,
  max_angle_deg: f32,
  max_offset: f32,
) -> Result<MeshHandle, ErrorStack> {
  verify_cgal_loaded()?;

  let raw_mesh = mesh.mesh.to_raw_indexed(false, false, true);

  assert_eq!(std::mem::size_of::<usize>(), std::mem::size_of::<u32>());
  let in_indices = unsafe {
    std::slice::from_raw_parts(
      raw_mesh.indices.as_ptr() as *const u32,
      raw_mesh.indices.len(),
    )
  };

  cgal_remesh_planar_patches(&raw_mesh.vertices, in_indices, max_angle_deg, max_offset);

  read_cgal_output_mesh(mesh.transform, mesh.material.clone())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn remesh_planar_patches(
  _mesh: &MeshHandle,
  _max_angle_deg: f32,
  _max_offset: f32,
) -> Result<MeshHandle, ErrorStack> {
  Err(ErrorStack::new(
    "Planar patch remeshing is not supported outside of wasm",
  ))
}

#[cfg(target_arch = "wasm32")]
pub fn isotropic_remesh(
  mesh: &MeshHandle,
  target_edge_length: f32,
  max_iterations: u32,
  protect_borders: bool,
  auto_sharp_edges: bool,
  sharp_angle_threshold_degrees: f32,
) -> Result<MeshHandle, ErrorStack> {
  verify_cgal_loaded()?;

  let raw_mesh = mesh.mesh.to_raw_indexed(false, false, true);

  assert_eq!(std::mem::size_of::<usize>(), std::mem::size_of::<u32>());
  let in_indices = unsafe {
    std::slice::from_raw_parts(
      raw_mesh.indices.as_ptr() as *const u32,
      raw_mesh.indices.len(),
    )
  };

  cgal_remesh_isotropic(
    &raw_mesh.vertices,
    in_indices,
    target_edge_length,
    max_iterations,
    protect_borders,
    auto_sharp_edges,
    sharp_angle_threshold_degrees,
  );

  read_cgal_output_mesh(mesh.transform, mesh.material.clone())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn isotropic_remesh(
  _mesh: &MeshHandle,
  _target_edge_length: f32,
  _max_iterations: u32,
  _protect_borders: bool,
  _auto_sharp_edges: bool,
  _sharp_angle_threshold_degrees: f32,
) -> Result<MeshHandle, ErrorStack> {
  Err(ErrorStack::new(
    "Isotropic remeshing is not supported outside of wasm",
  ))
}

#[cfg(target_arch = "wasm32")]
pub fn delaunay_remesh(
  mesh: &MeshHandle,
  target_edge_length: f32,
  facet_distance: f32,
  auto_sharp_edges: bool,
  sharp_angle_threshold_degrees: f32,
) -> Result<MeshHandle, ErrorStack> {
  verify_cgal_loaded()?;

  let raw_mesh = mesh.mesh.to_raw_indexed(false, false, true);

  assert_eq!(std::mem::size_of::<usize>(), std::mem::size_of::<u32>());
  let in_indices = unsafe {
    std::slice::from_raw_parts(
      raw_mesh.indices.as_ptr() as *const u32,
      raw_mesh.indices.len(),
    )
  };

  cgal_remesh_delaunay(
    &raw_mesh.vertices,
    in_indices,
    target_edge_length,
    facet_distance,
    auto_sharp_edges,
    sharp_angle_threshold_degrees,
  );

  // sometimes the normals get flipped in delaunay remeshing; re-orient connected components
  // outward.
  //
  // This kinda works, the only for the components that are manifold - and manifoldness isn't
  // guaranteed.
  let mut out_mesh = read_raw_cgal_output_mesh()?;
  out_mesh.orient_connected_components_outward();

  Ok(MeshHandle {
    mesh: Rc::new(out_mesh),
    transform: mesh.transform,
    manifold_handle: mesh.manifold_handle.clone(),
    aabb: mesh.aabb.clone(),
    trimesh: mesh.trimesh.clone(),
    material: mesh.material.clone(),
  })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn delaunay_remesh(
  _mesh: &MeshHandle,
  _target_edge_length: f32,
  _facet_distance: f32,
  _auto_sharp_edges: bool,
  _sharp_angle_threshold_degrees: f32,
) -> Result<MeshHandle, ErrorStack> {
  Err(ErrorStack::new(
    "Delaunay remeshing is not supported outside of wasm",
  ))
}

/// Information about an edge for filtering in `bevel_mesh`.
pub struct EdgeBevelInfo {
  /// Position of the first vertex of the edge
  pub v0_pos: Vec3,
  /// Position of the second vertex of the edge
  pub v1_pos: Vec3,
  /// Dihedral angle in radians between the two adjacent faces.
  /// For non-manifold edges (more than 2 faces), this is the maximum angle between any pair.
  /// For boundary edges (1 face), this is PI (180 degrees).
  pub dihedral_angle: f32,
}

/// Computes the dihedral angle between two face normals sharing an edge.
/// Returns angle in radians in range [0, PI].
fn compute_dihedral_angle(n0: Vec3, n1: Vec3) -> f32 {
  // The dihedral angle is the angle between the face normals.
  // For convex edges, this is < PI, for concave edges > PI.
  // However, we use the unsigned angle here.
  let dot = n0.dot(&n1).clamp(-1.0, 1.0);
  dot.acos()
}

#[cfg(target_arch = "wasm32")]
pub fn bevel_mesh<F>(
  mesh: &MeshHandle,
  inset_amount: f32,
  subdivision_levels: u32,
  mut edge_filter: F,
) -> Result<MeshHandle, ErrorStack>
where
  F: FnMut(&EdgeBevelInfo) -> Result<bool, ErrorStack>,
{
  verify_cgal_loaded()?;

  let (raw_mesh, vtx_key_to_ix) = mesh.mesh.to_raw_indexed_with_mapping(false, false, true);

  assert_eq!(std::mem::size_of::<usize>(), std::mem::size_of::<u32>());
  let in_indices = unsafe {
    std::slice::from_raw_parts(
      raw_mesh.indices.as_ptr() as *const u32,
      raw_mesh.indices.len(),
    )
  };

  // Iterate all edges in the mesh and determine which to bevel
  let mut edges_to_bevel: Vec<u32> = Vec::new();

  for (_edge_key, edge) in mesh.mesh.iter_edges() {
    let [v0_key, v1_key] = edge.vertices;
    let v0_pos = mesh.mesh.vertices[v0_key].position;
    let v1_pos = mesh.mesh.vertices[v1_key].position;

    // Compute dihedral angle
    let dihedral_angle = if edge.faces.is_empty() {
      // Isolated edge (shouldn't happen normally), treat as sharp
      std::f32::consts::PI
    } else if edge.faces.len() == 1 {
      // Boundary edge, treat as sharp
      std::f32::consts::PI
    } else {
      // Compute max dihedral angle across all face pairs
      let mut max_angle: f32 = 0.0;
      for i in 0..edge.faces.len() {
        let face_i = &mesh.mesh.faces[edge.faces[i]];
        let normal_i = face_i.normal(&mesh.mesh.vertices);
        for j in (i + 1)..edge.faces.len() {
          let face_j = &mesh.mesh.faces[edge.faces[j]];
          let normal_j = face_j.normal(&mesh.mesh.vertices);
          let angle = compute_dihedral_angle(normal_i, normal_j);
          max_angle = max_angle.max(angle);
        }
      }
      max_angle
    };

    let info = EdgeBevelInfo {
      v0_pos,
      v1_pos,
      dihedral_angle,
    };

    if edge_filter(&info)? {
      let Some(&v0_ix) = vtx_key_to_ix.get(&v0_key) else {
        // Vertex not in the indexed mesh (e.g., isolated vertex)
        continue;
      };
      let Some(&v1_ix) = vtx_key_to_ix.get(&v1_key) else {
        continue;
      };

      let (v0_ix, v1_ix) = if v0_ix < v1_ix {
        (v0_ix as u32, v1_ix as u32)
      } else {
        (v1_ix as u32, v0_ix as u32)
      };

      edges_to_bevel.push(v0_ix);
      edges_to_bevel.push(v1_ix);
    }
  }

  cgal_bevel_mesh(
    &raw_mesh.vertices,
    in_indices,
    &edges_to_bevel,
    inset_amount,
    subdivision_levels,
  );

  read_cgal_output_mesh(mesh.transform, mesh.material.clone())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn bevel_mesh<F>(
  _mesh: &MeshHandle,
  _inset_amount: f32,
  _subdivision_levels: u32,
  _edge_filter: F,
) -> Result<MeshHandle, ErrorStack>
where
  F: FnMut(&EdgeBevelInfo) -> Result<bool, ErrorStack>,
{
  Err(ErrorStack::new(
    "Mesh beveling is not supported outside of wasm",
  ))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/geoscript/text_to_path")]
extern "C" {
  pub fn get_cached_text_to_path_verts(
    text: &str,
    font_family: &str,
    font_size: f32,
    font_weight: &str,
    font_style: &str,
    letter_spacing: f32,
    width: f32,
    height: f32,
  ) -> Option<Vec<f32>>;
  pub fn get_cached_text_to_path_indices(
    text: &str,
    font_family: &str,
    font_size: f32,
    font_weight: &str,
    font_style: &str,
    letter_spacing: f32,
    width: f32,
    height: f32,
  ) -> Option<Vec<u32>>;
  pub fn get_cached_text_to_path_err(
    text: &str,
    font_family: &str,
    font_size: f32,
    font_weight: &str,
    font_style: &str,
    letter_spacing: f32,
    width: f32,
    height: f32,
  ) -> Option<String>;
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn get_text_to_path_cached_mesh(
  text: &str,
  font_family: &str,
  font_size: f32,
  font_weight: &str,
  font_style: &str,
  letter_spacing: f32,
  width: f32,
  height: f32,
  depth: Option<f32>,
) -> Result<Option<LinkedMesh<()>>, ErrorStack> {
  let maybe_err = get_cached_text_to_path_err(
    text,
    font_family,
    font_size,
    font_weight,
    font_style,
    letter_spacing,
    width,
    height,
  );
  if let Some(err_str) = maybe_err {
    return Err(ErrorStack::new(err_str));
  }

  let maybe_verts = get_cached_text_to_path_verts(
    text,
    font_family,
    font_size,
    font_weight,
    font_style,
    letter_spacing,
    width,
    height,
  );
  let maybe_indices = get_cached_text_to_path_indices(
    text,
    font_family,
    font_size,
    font_weight,
    font_style,
    letter_spacing,
    width,
    height,
  );

  // convert from 2d vertices to 3d vertices in the XZ plane
  let maybe_verts = maybe_verts.map(|verts_2d| {
    let mut verts_3d = Vec::with_capacity(verts_2d.len() / 2 * 3);
    for i in 0..(verts_2d.len() / 2) {
      verts_3d.push(verts_2d[i * 2]);
      verts_3d.push(0.);
      verts_3d.push(verts_2d[i * 2 + 1]);
    }
    verts_3d
  });

  let mut mesh = match (maybe_verts, maybe_indices) {
    (Some(verts), Some(indices)) => LinkedMesh::from_raw_indexed(&verts, &indices, None, None),
    _ => return Ok(None),
  };

  let Some(depth) = depth else {
    return Ok(Some(mesh));
  };

  crate::mesh_ops::extrude::extrude(&mut mesh, |_| Ok(Vec3::new(0., depth, 0.)))?;
  Ok(Some(mesh))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn get_text_to_path_cached_mesh(
  _text: &str,
  _font_family: &str,
  _font_size: f32,
  _font_weight: &str,
  _font_style: &str,
  _letter_spacing: f32,
  _width: f32,
  _height: f32,
  _depth: Option<f32>,
) -> Result<Option<LinkedMesh<()>>, ErrorStack> {
  Err(ErrorStack::new(
    "Text to path mesh generation is not supported outside of wasm",
  ))
}
