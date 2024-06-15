use common::mesh::{
  linked_mesh::{set_debug_print, set_graphviz_print},
  LinkedMesh, OwnedIndexedMesh,
};
use noise::{MultiFractal, NoiseModule};
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

#[repr(u32)]
enum DisplacementMethod {
  None = 0,
  Noise = 1,
  Constant = 2,
}

impl TryFrom<u32> for DisplacementMethod {
  type Error = ();

  fn try_from(value: u32) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(DisplacementMethod::None),
      1 => Ok(DisplacementMethod::Noise),
      2 => Ok(DisplacementMethod::Constant),
      _ => Err(()),
    }
  }
}

fn displace_mesh(mesh: &mut LinkedMesh, method: DisplacementMethod) {
  match method {
    DisplacementMethod::None => {}
    DisplacementMethod::Noise => {
      let noise = noise::Fbm::new().set_octaves(4);
      for (_vtx_key, vtx) in &mut mesh.vertices {
        let pos = vtx.position * 0.2;
        let noise = noise.get([pos.x, pos.y, pos.z]).abs();
        let displacement_normal = vtx
          .displacement_normal
          .expect("Expected displacement normal to be set by now");
        vtx.position += displacement_normal * noise * 1.8;
      }
    }
    DisplacementMethod::Constant => {
      for (_vtx_key, vtx) in &mut mesh.vertices {
        let displacement_normal = vtx
          .displacement_normal
          .expect("Expected displacement normal to be set by now");
        vtx.position += displacement_normal * 0.4;
      }
    }
  }
}

#[wasm_bindgen]
pub fn tessellate_mesh(
  vertices: &[f32],
  indices: &[usize],
  target_edge_length: f32,
  sharp_edge_threshold_rads: f32,
  displacement_method: u32,
) -> *mut TessellateMeshCtx {
  maybe_init();

  let displacement_method = DisplacementMethod::try_from(displacement_method).unwrap();

  let mut mesh = LinkedMesh::from_raw_indexed(vertices, indices, None, None);

  let removed_vert_count = mesh.merge_vertices_by_distance(0.0001);
  info!("Removed {removed_vert_count} vertices from merge by distance");
  mesh.separate_vertices_and_compute_normals(sharp_edge_threshold_rads);
  crate::tessellate_mesh(&mut mesh, target_edge_length);

  displace_mesh(&mut mesh, displacement_method);

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
pub fn tessellate_mesh_ctx_has_shading_normals(ctx: *const TessellateMeshCtx) -> bool {
  unsafe { (*ctx).new_mesh.shading_normals.is_some() }
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_has_displacement_normals(ctx: *const TessellateMeshCtx) -> bool {
  unsafe { (*ctx).new_mesh.displacement_normals.is_some() }
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_get_shading_normals(ctx: *const TessellateMeshCtx) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let normals = ctx.new_mesh.shading_normals.as_ref();
  if let Some(normals) = normals {
    normals.clone()
  } else {
    Vec::new()
  }
}

#[wasm_bindgen]
pub fn tessellate_mesh_ctx_get_displacement_normals(ctx: *const TessellateMeshCtx) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let normals = ctx.new_mesh.displacement_normals.as_ref();
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
