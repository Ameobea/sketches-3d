use wasm_bindgen::prelude::*;

use mesh::{
  csg::{FaceData, CSG},
  LinkedMesh, OwnedIndexedMesh,
};

static mut DID_INIT: bool = false;

fn maybe_init() {
  unsafe {
    if DID_INIT {
      return;
    }
    DID_INIT = true;
  }

  console_error_panic_hook::set_once();
  wasm_logger::init(wasm_logger::Config::new(log::Level::Debug));
}

pub struct CsgSandboxCtx {
  mesh: OwnedIndexedMesh,
}

#[wasm_bindgen]
pub fn create_mesh(indices: &[u32], vertices: &[f32]) -> *mut LinkedMesh<FaceData> {
  maybe_init();

  let mut mesh = LinkedMesh::from_raw_indexed(vertices, indices, None, None);
  mesh.merge_vertices_by_distance(1e-5);
  mesh.mark_edge_sharpness(0.8);
  mesh.separate_vertices_and_compute_normals();

  Box::into_raw(Box::new(mesh))
}

#[wasm_bindgen]
pub fn free_mesh(mesh: *mut LinkedMesh<FaceData>) {
  drop(unsafe { Box::from_raw(mesh) });
}

#[wasm_bindgen]
pub fn csg_sandbox_init(
  mesh_0: *const LinkedMesh<FaceData>,
  mesh_1: *const LinkedMesh<FaceData>,
) -> *mut CsgSandboxCtx {
  let mesh0 = unsafe { &*mesh_0 }.clone();
  let mesh1 = unsafe { &*mesh_1 }.clone();

  let csg0 = CSG::from(mesh0);
  let csg1 = CSG::from(mesh1);
  let mesh = csg0.subtract(csg1.mesh);

  // let sharp_edge_threshold_rads = 0.8;
  // mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  // mesh.separate_vertices_and_compute_normals();

  let mesh = mesh.to_raw_indexed(true, true);
  Box::into_raw(Box::new(CsgSandboxCtx { mesh }))
}

#[wasm_bindgen]
pub fn csg_sandbox_free(ctx: *mut CsgSandboxCtx) {
  drop(unsafe { Box::from_raw(ctx) });
}

#[wasm_bindgen]
pub fn csg_sandbox_take_indices(ctx: *mut CsgSandboxCtx) -> Vec<usize> {
  let ctx = unsafe { &mut *ctx };
  std::mem::take(&mut ctx.mesh.indices)
}

#[wasm_bindgen]
pub fn csg_sandbox_take_vertices(ctx: *mut CsgSandboxCtx) -> Vec<f32> {
  let ctx = unsafe { &mut *ctx };
  std::mem::take(&mut ctx.mesh.vertices)
}

#[wasm_bindgen]
pub fn csg_sandbox_take_normals(ctx: *mut CsgSandboxCtx) -> Vec<f32> {
  let ctx = unsafe { &mut *ctx };
  std::mem::take(ctx.mesh.shading_normals.as_mut().expect("no normals"))
}

// TODO TEMP
#[wasm_bindgen]
pub fn csg_sandbox_take_displacement_normals(ctx: *mut CsgSandboxCtx) -> Vec<f32> {
  let ctx = unsafe { &mut *ctx };
  std::mem::take(
    ctx
      .mesh
      .displacement_normals
      .as_mut()
      .expect("no displacement normals"),
  )
}
