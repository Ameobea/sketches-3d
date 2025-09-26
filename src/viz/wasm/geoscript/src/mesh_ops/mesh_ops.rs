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
fn read_cgal_output_mesh(
  transform: Mat4,
  material: Option<Rc<crate::Material>>,
) -> Result<MeshHandle, String> {
  use crate::ManifoldHandle;
  use std::cell::RefCell;

  let out_verts = cgal_get_output_mesh_verts();
  let out_indices = cgal_get_output_mesh_indices();
  cgal_clear_output_mesh();

  let out_mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(&out_verts, &out_indices, None, None);
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
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasm/cgal/cgal")]
extern "C" {
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
pub fn simplify_mesh(mesh: &MeshHandle, tolerance: f32) -> Result<MeshHandle, ErrorStack> {
  use std::cell::RefCell;

  use crate::ManifoldHandle;

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
pub fn convex_hull_from_verts(verts: &[Vec3]) -> Result<MeshHandle, String> {
  use std::cell::RefCell;

  use crate::ManifoldHandle;

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
pub fn convex_hull_from_verts(_verts: &[Vec3]) -> Result<MeshHandle, String> {
  Ok(MeshHandle::new(Rc::new(LinkedMesh::new(0, 0, None))))
}

#[cfg(target_arch = "wasm32")]
pub fn split_mesh_by_plane(
  mesh: &MeshHandle,
  plane_normal: Vec3,
  plane_offset: f32,
) -> Result<(MeshHandle, MeshHandle), ErrorStack> {
  use crate::ManifoldHandle;

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
) -> Result<MeshHandle, String> {
  let raw_mesh = mesh.mesh.to_raw_indexed(false, false, true);

  assert_eq!(std::mem::size_of::<u32>(), std::mem::size_of::<u32>());
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
) -> Result<MeshHandle, String> {
  Err("Alpha wrapping is not supported outside of wasm".to_string())
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
) -> Result<MeshHandle, String> {
  let raw_mesh = mesh.mesh.to_raw_indexed(false, false, true);

  assert_eq!(std::mem::size_of::<u32>(), std::mem::size_of::<u32>());
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
) -> Result<MeshHandle, String> {
  Err("Mesh smoothing is not supported outside of wasm".to_string())
}

#[cfg(target_arch = "wasm32")]
pub fn alpha_wrap_points(
  points: &[Vec3],
  relative_alpha: f32,
  relative_offset: f32,
) -> Result<MeshHandle, String> {
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
) -> Result<MeshHandle, String> {
  Err("Alpha wrapping is not supported outside of wasm".to_string())
}
