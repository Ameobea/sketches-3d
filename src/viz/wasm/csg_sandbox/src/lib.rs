use wasm_bindgen::prelude::*;

use geoscript::mesh_ops::mesh_boolean::{apply_boolean, decode_manifold_output, MeshBooleanOp};
use mesh::{csg::FaceData, linked_mesh::DisplacementNormalMethod, LinkedMesh, OwnedIndexedMesh};

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
pub fn create_mesh(indices: Vec<u32>, vertices: Vec<f32>) -> *mut LinkedMesh<FaceData> {
  maybe_init();

  let mut mesh = LinkedMesh::from_raw_indexed(&vertices, &indices, None, None);
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

fn displace_mesh<T>(mesh: &mut LinkedMesh<T>) {
  use noise::{MultiFractal, NoiseModule};

  let noise = noise::Fbm::new().set_octaves(4);
  for (_vtx_key, vtx) in &mut mesh.vertices {
    let pos = vtx.position * 0.2;
    let noise = noise.get([pos.x, pos.y, pos.z]); //.abs();
    let displacement_normal = vtx
      .displacement_normal
      .expect("Expected displacement normal to be set by now");
    vtx.position += displacement_normal * noise * 1.8;
  }

  // for (_vtx_key, vtx) in &mut mesh.vertices {
  //   let displacement_normal = vtx
  //     .displacement_normal
  //     .expect("Expected displacement normal to be set by now");
  //   vtx.position += displacement_normal * 0.3;
  // }
}

#[wasm_bindgen]
pub fn csg_sandbox_init(
  mesh_0: *const LinkedMesh<FaceData>,
  mesh_1: *const LinkedMesh<FaceData>,
) -> *mut CsgSandboxCtx {
  let mesh0 = unsafe { &*mesh_0 };
  let mesh1 = unsafe { &*mesh_1 };
  let mut mesh1 = mesh1.clone();

  // let csg0 = CSG::from(mesh0.mesh.clone());
  // let csg1 = CSG::from(mesh1.mesh.clone());

  // let mut mesh = csg0.subtract(csg1.mesh);
  // let mut mesh = csg0
  //   .intersect_experimental(csg1.mesh)
  //   .expect("Error applying CSG");

  let target_edge_length = 0.56;
  tessellation::tessellate_mesh(
    &mut mesh1,
    target_edge_length,
    DisplacementNormalMethod::Interpolate,
  );

  mesh1.compute_vertex_displacement_normals();
  displace_mesh(&mut mesh1);

  let mesh0_exported = mesh0.to_raw_indexed(false, false, false);
  let mesh1_exported = mesh1.to_raw_indexed(false, false, false);

  assert!(std::mem::size_of::<u32>() == std::mem::size_of::<usize>());
  let mesh0_exported_indices = unsafe {
    std::slice::from_raw_parts(
      mesh0_exported.indices.as_ptr() as *const u32,
      mesh0_exported.indices.len(),
    )
  };
  let mesh1_exported_indices = unsafe {
    std::slice::from_raw_parts(
      mesh1_exported.indices.as_ptr() as *const u32,
      mesh1_exported.indices.len(),
    )
  };

  let encoded_output = apply_boolean(
    &mesh0_exported.vertices,
    &mesh0_exported_indices,
    &mesh1_exported.vertices,
    &mesh1_exported_indices,
    MeshBooleanOp::Difference as u8,
  );
  let (out_verts, out_indices) = decode_manifold_output(&encoded_output);
  let mut mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(&out_verts, &out_indices, None, None);
  mesh
    .check_is_manifold::<true>()
    .expect("Mesh is not manifold after CSG operation");

  mesh.merge_vertices_by_distance(1e-3);
  let sharp_edge_threshold_rads = 0.8;
  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_vertex_displacement_normals();
  // mesh.separate_vertices_and_compute_normals();

  // let target_edge_length = 0.56;
  // tessellation::tessellate_mesh(
  //   &mut mesh,
  //   target_edge_length,
  //   DisplacementNormalMethod::Interpolate,
  // );

  // displace_mesh(&mut mesh);

  // mesh.merge_vertices_by_distance(1e-5);

  // mesh
  //   .check_is_manifold::<true>()
  //   .expect("Mesh is not manifold after tessellation and displacement");

  // let mesh_exported = mesh.to_raw_indexed(false, false);
  // let mesh_exported_indices = unsafe {
  //   std::slice::from_raw_parts(
  //     mesh_exported.indices.as_ptr() as *const u32,
  //     mesh_exported.indices.len(),
  //   )
  // };

  // let mut mesh1_exported_vertices_offset = mesh1_exported.vertices.clone();
  // for vtx in mesh1_exported_vertices_offset.chunks_exact_mut(3) {
  //   vtx[0] += 15.5;
  // }
  // let encoded_output = apply_boolean(
  //   &mesh_exported.vertices,
  //   &mesh_exported_indices,
  //   &mesh1_exported_vertices_offset,
  //   &mesh1_exported_indices,
  //   BooleanOp::Difference as u8,
  // );
  // let (out_verts, out_indices) = decode_manifold_output(&encoded_output);
  // let mut mesh: LinkedMesh<()> = LinkedMesh::from_raw_indexed(&out_verts, &out_indices, None,
  // None); mesh
  //   .check_is_manifold::<true>()
  //   .expect("Mesh is not manifold after CSG operation");

  mesh.mark_edge_sharpness(sharp_edge_threshold_rads);
  mesh.compute_edge_displacement_normals();
  mesh.separate_vertices_and_compute_normals();

  let mesh = mesh.to_raw_indexed(true, true, false);
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
