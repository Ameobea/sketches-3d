use common::mesh::{
  linked_mesh::{set_debug_print, set_graphviz_print},
  LinkedMesh, OwnedIndexedMesh,
};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(module = "src/viz/scenes/tessellationSandbox/graphvizDebug")]
extern "C" {
  fn build_linked_mesh_graphviz(s: &str) -> String;
}

pub struct TessellateMeshCtx {
  new_mesh: OwnedIndexedMesh,
}

static mut DID_INIT: bool = false;

fn maybe_init() {
  unsafe {
    if DID_INIT {
      return;
    }
    DID_INIT = true;
  }

  set_graphviz_print(graphviz_print);
  set_debug_print(debug_print);
  console_error_panic_hook::set_once();
  wasm_logger::init(wasm_logger::Config::new(log::Level::Debug));
}

fn graphviz_print(s: &str) {
  build_linked_mesh_graphviz(s);
}

fn debug_print(s: &str) {
  info!("{s}");
}

#[wasm_bindgen]
pub fn tessellate_mesh(
  vertices: &[f32],
  vertex_normals: &[f32],
  indices: &[usize],
  target_triangle_area: f32,
) -> *mut TessellateMeshCtx {
  maybe_init();

  let mut mesh = LinkedMesh::from_raw_indexed(
    vertices,
    indices,
    if vertex_normals.is_empty() {
      None
    } else {
      Some(vertex_normals)
    },
    None,
  );

  let removed_vert_count = mesh.merge_vertices_by_distance(std::f32::EPSILON);
  info!("Removed {removed_vert_count} vertices from merge by distance");
  crate::tessellate_mesh(&mut mesh, target_triangle_area);
  mesh.compute_vertex_normals(0.8);
  let ctx = Box::new(TessellateMeshCtx {
    new_mesh: mesh.to_raw_indexed(),
  });

  Box::into_raw(ctx)
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_free(ctx: *mut TessellateMeshCtx) {
  drop(unsafe { Box::from_raw(ctx) });
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_get_vertices(ctx: *const TessellateMeshCtx) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  ctx.new_mesh.vertices.clone()
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
    normals.clone()
  } else {
    Vec::new()
  }
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_get_indices(ctx: *const TessellateMeshCtx) -> Vec<usize> {
  let ctx = unsafe { &*ctx };
  ctx.new_mesh.indices.clone()
}
