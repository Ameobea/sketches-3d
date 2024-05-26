use common::mesh::{Mesh, OwnedMesh};
use wasm_bindgen::prelude::*;

pub struct TessellateMeshCtx {
  new_mesh: OwnedMesh,
}

#[wasm_bindgen]
pub fn tessellate_mesh(
  vertices: &[f32],
  vertex_normals: &[f32],
  target_triangle_area: f32,
) -> *mut TessellateMeshCtx {
  console_error_panic_hook::set_once();

  let mesh = Mesh::from_raw(vertices, vertex_normals, None);
  let new_mesh = crate::tessellate_mesh(mesh, target_triangle_area);
  let ctx = Box::new(TessellateMeshCtx { new_mesh });
  Box::into_raw(ctx)
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_free(ctx: *mut TessellateMeshCtx) {
  drop(unsafe { Box::from_raw(ctx) });
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_get_vertices(ctx: *const TessellateMeshCtx) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let vertices = unsafe {
    std::slice::from_raw_parts(
      ctx.new_mesh.vertices.as_ptr() as *const f32,
      ctx.new_mesh.vertices.len() * 3,
    )
  };
  vertices.to_owned()
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_has_normals(ctx: *const TessellateMeshCtx) -> bool {
  unsafe { (*ctx).new_mesh.normals.is_some() }
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_get_normals(ctx: *const TessellateMeshCtx) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let normals = ctx.new_mesh.normals.as_ref();
  if let Some(normals) = normals {
    let normals =
      unsafe { std::slice::from_raw_parts(normals.as_ptr() as *const f32, normals.len() * 3) };
    normals.to_owned()
  } else {
    Vec::new()
  }
}
