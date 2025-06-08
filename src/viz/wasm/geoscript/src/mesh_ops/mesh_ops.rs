use mesh::linked_mesh::Vec3;
use mesh::LinkedMesh;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(module = "src/viz/wasmComp/manifold")]
extern "C" {
  pub fn simplify(verts: &[f32], indices: &[u32], tolerance: f32) -> Vec<u8>;
  pub fn convex_hull(verts: &[f32]) -> Vec<u8>;
}

#[cfg(target_arch = "wasm32")]
pub fn simplify_mesh(mesh: &LinkedMesh<()>, tolerance: f32) -> Result<LinkedMesh<()>, String> {
  use crate::mesh_ops::mesh_boolean::decode_manifold_output;

  if tolerance <= 0. {
    return Err("Tolerance must be greater than zero".to_owned());
  }

  let raw_mesh = mesh.to_raw_indexed(false, false, true);
  assert!(std::mem::size_of::<u32>() == std::mem::size_of::<usize>());
  let indices = unsafe {
    std::slice::from_raw_parts(
      raw_mesh.indices.as_ptr() as *const u32,
      raw_mesh.indices.len(),
    )
  };
  let verts = &raw_mesh.vertices;

  let encoded_output = simplify(verts, indices, tolerance);

  let (out_verts, out_indices) = decode_manifold_output(&encoded_output);
  Ok(LinkedMesh::from_raw_indexed(
    out_verts,
    out_indices,
    None,
    None,
  ))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn simplify_mesh(mesh: &LinkedMesh<()>, _tolerance: f32) -> Result<LinkedMesh<()>, String> {
  Ok(mesh.clone())
}

#[cfg(target_arch = "wasm32")]
pub fn convex_hull_from_verts(verts: &[Vec3]) -> Result<LinkedMesh<()>, String> {
  use crate::mesh_ops::mesh_boolean::decode_manifold_output;

  let verts = unsafe { std::slice::from_raw_parts(verts.as_ptr() as *const f32, verts.len() * 3) };

  let encoded_output = convex_hull(verts);
  let (out_verts, out_indices) = decode_manifold_output(&encoded_output);

  Ok(LinkedMesh::from_raw_indexed(
    out_verts,
    out_indices,
    None,
    None,
  ))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn convex_hull_from_verts(_verts: &[Vec3]) -> Result<LinkedMesh<()>, String> {
  Ok(LinkedMesh::new(0, 0, None))
}
