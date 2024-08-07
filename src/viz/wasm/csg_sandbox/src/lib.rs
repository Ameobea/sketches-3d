use wasm_bindgen::prelude::*;

use mesh::{
  csg::{CSG, INTERIOR_VTX_POSITIONS},
  linked_mesh::Vec3,
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

fn displace_mesh(mesh: &mut LinkedMesh) {
  // for vtx in &mut mesh.vertices.values_mut() {
  //   vtx.position += vtx
  //     .displacement_normal
  //     .expect("Missing displacement normal")
  //     * 0.5;
  // }
}

fn cubes() -> LinkedMesh {
  let csg0 = CSG::new_cube(Vec3::zeros(), 4.);
  let csg1 = CSG::new_cube(Vec3::new(3.5, 3.5, 3.5), 4.);

  csg0.subtract(csg1.mesh)
}

#[wasm_bindgen]
pub fn csg_sandbox_init(indices: &[u32], vertices: &[f32]) -> *mut CsgSandboxCtx {
  maybe_init();

  // let mut mesh = cubes();
  let mesh = LinkedMesh::from_raw_indexed(&vertices, indices, None, None);
  let csg0 = CSG::from(mesh);
  let csg1 = CSG::new_cube(Vec3::new(3.2435, 3.523, 3.59756), 4.);
  let mut mesh = csg1.subtract(csg0.mesh);
  mesh.cleanup_degenerate_triangles();
  mesh.merge_vertices_by_distance(1e-5);

  let sharp_edge_threshold_rads = 0.8;
  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);

  mesh.compute_vertex_displacement_normals();
  displace_mesh(&mut mesh);

  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.separate_vertices_and_compute_normals();
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

// TODO TEMP
#[wasm_bindgen]
pub fn csg_sandbox_take_interior_vtx_positions() -> Vec<f32> {
  unsafe { std::mem::take(&mut *INTERIOR_VTX_POSITIONS) }
}
