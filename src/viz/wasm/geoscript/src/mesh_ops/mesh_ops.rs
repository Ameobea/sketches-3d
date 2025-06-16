use mesh::linked_mesh::Vec3;
use mesh::LinkedMesh;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

use crate::MeshHandle;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasmComp/manifold")]
extern "C" {
  pub fn simplify(handle: usize, tolerance: f32) -> Vec<u8>;
  pub fn convex_hull(verts: &[f32]) -> Vec<u8>;
}

#[cfg(target_arch = "wasm32")]
pub fn simplify_mesh(mesh: &MeshHandle, tolerance: f32) -> Result<MeshHandle, String> {
  use std::sync::Arc;

  use nalgebra::Matrix4;

  use crate::ManifoldHandle;

  if tolerance <= 0. {
    return Err("Tolerance must be greater than zero".to_owned());
  }

  let encoded_output = simplify(mesh.get_or_create_handle(), tolerance);

  let (manifold_handle, out_verts, out_indices) =
    crate::mesh_ops::mesh_boolean::decode_manifold_output(&encoded_output);
  let out_mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(out_verts, out_indices, None, None);
  Ok(MeshHandle {
    mesh: Arc::new(out_mesh),
    transform: Box::new(Matrix4::identity()),
    manifold_handle: Arc::new(ManifoldHandle::new(manifold_handle)),
  })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn simplify_mesh(mesh: &MeshHandle, _tolerance: f32) -> Result<MeshHandle, String> {
  Ok(mesh.clone())
}

#[cfg(target_arch = "wasm32")]
pub fn convex_hull_from_verts(verts: &[Vec3]) -> Result<MeshHandle, String> {
  use std::sync::Arc;

  use nalgebra::Matrix4;

  use crate::ManifoldHandle;

  let verts = unsafe { std::slice::from_raw_parts(verts.as_ptr() as *const f32, verts.len() * 3) };

  let encoded_output = convex_hull(verts);
  let (manifold_handle, out_verts, out_indices) =
    crate::mesh_ops::mesh_boolean::decode_manifold_output(&encoded_output);

  let out_mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(out_verts, out_indices, None, None);
  Ok(MeshHandle {
    mesh: Arc::new(out_mesh),
    transform: Box::new(Matrix4::identity()),
    manifold_handle: Arc::new(ManifoldHandle::new(manifold_handle)),
  })
}

#[cfg(not(target_arch = "wasm32"))]
pub fn convex_hull_from_verts(_verts: &[Vec3]) -> Result<MeshHandle, String> {
  Ok(MeshHandle::new(std::sync::Arc::new(LinkedMesh::new(
    0, 0, None,
  ))))
}
