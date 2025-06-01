use noise::{MultiFractal, NoiseModule};
use wasm_bindgen::prelude::*;

use mesh::{
  csg::{FaceData, CSG},
  linked_mesh::DisplacementNormalMethod,
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
  wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
}

pub struct CsgSandboxCtx {
  mesh: OwnedIndexedMesh,
}

#[wasm_bindgen]
pub fn create_mesh(indices: &[u32], vertices: &[f32]) -> *mut LinkedMesh<FaceData> {
  maybe_init();

  let mut mesh = LinkedMesh::from_raw_indexed(vertices, indices, None, None);
  mesh.merge_vertices_by_distance(1e-5);
  mesh
    .check_is_manifold::<true>()
    .expect("Mesh is not manifold");
  // mesh.mark_edge_sharpness(0.8);
  // mesh.separate_vertices_and_compute_normals();

  Box::into_raw(Box::new(mesh))
}

#[wasm_bindgen]
pub fn free_mesh(mesh: *mut LinkedMesh<FaceData>) {
  drop(unsafe { Box::from_raw(mesh) });
}

fn displace_mesh(mesh: &mut LinkedMesh) {
  // let noise = noise::Fbm::new().set_octaves(4);
  // for (_vtx_key, vtx) in &mut mesh.vertices {
  //   let pos = vtx.position * 0.2;
  //   let noise = noise.get([pos.x, pos.y, pos.z]); //.abs();
  //   let displacement_normal = vtx
  //     .displacement_normal
  //     .expect("Expected displacement normal to be set by now");
  //   vtx.position += displacement_normal * noise * 1.8;
  // }

  for (_vtx_key, vtx) in &mut mesh.vertices {
    let displacement_normal = vtx
      .displacement_normal
      .expect("Expected displacement normal to be set by now");
    vtx.position += displacement_normal * 0.3;
  }
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
  // let mut mesh = csg0.subtract(csg1.mesh);
  let mut mesh = csg0
    .intersect_experimental(csg1.mesh)
    .expect("Error applying CSG");

  mesh.merge_vertices_by_distance(1e-3);
  let sharp_edge_threshold_rads = 0.8;
  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_vertex_displacement_normals();
  // mesh.separate_vertices_and_compute_normals();

  let target_edge_length = 4.26;
  tessellation::tessellate_mesh(
    &mut mesh,
    target_edge_length,
    DisplacementNormalMethod::Interpolate,
  );

  displace_mesh(&mut mesh);

  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_edge_displacement_normals();
  mesh.separate_vertices_and_compute_normals();

  let mut deleted_tri_count = 0;
  mesh.cleanup_degenerate_triangles_cb(|_, _| {
    deleted_tri_count += 1;
  });
  log::info!("deleted {deleted_tri_count} degenerate triangles");

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
