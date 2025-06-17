use geoscript::{
  mesh_ops::mesh_boolean::drop_all_mesh_handles, parse_and_eval_program_with_ctx, ErrorStack,
  EvalCtx,
};
use mesh::OwnedIndexedMesh;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
  #[wasm_bindgen(js_namespace = console)]
  fn log(s: &str);
}

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

pub struct GeoscriptReplCtx {
  pub geo_ctx: EvalCtx,
  pub last_result: Result<(), ErrorStack>,
  pub output_meshes: Vec<OwnedIndexedMesh>,
}

impl Default for GeoscriptReplCtx {
  fn default() -> Self {
    Self {
      geo_ctx: EvalCtx::default().set_log_fn(log),
      last_result: Ok(()),
      output_meshes: Vec::new(),
    }
  }
}

impl GeoscriptReplCtx {
  pub fn convert_rendered_meshes(&mut self) {
    self.output_meshes.clear();

    for mesh_handle in self.geo_ctx.rendered_meshes.inner.borrow_mut().drain(..) {
      let mut mesh = (*mesh_handle.mesh).clone();

      let merged_count = mesh.merge_vertices_by_distance(0.0001);
      if merged_count > 0 {
        ::log::info!("Merged {} vertices in mesh", merged_count);
      }
      mesh.mark_edge_sharpness(0.8);
      mesh.separate_vertices_and_compute_normals();

      let mut owned_mesh = mesh.to_raw_indexed(true, false, false);
      owned_mesh.transform = Some(mesh_handle.transform);
      self.output_meshes.push(owned_mesh);
    }
  }
}

#[wasm_bindgen]
pub fn geoscript_repl_init() -> *mut GeoscriptReplCtx {
  maybe_init();

  Box::into_raw(Box::new(GeoscriptReplCtx::default()))
}

#[wasm_bindgen]
pub fn geoscript_repl_eval(ctx: *mut GeoscriptReplCtx, src: &str) {
  let ctx = unsafe { &mut *ctx };
  ctx.last_result = parse_and_eval_program_with_ctx(src, &ctx.geo_ctx);
  ctx.convert_rendered_meshes();
}

#[wasm_bindgen]
pub fn geoscript_repl_reset(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };
  *ctx = GeoscriptReplCtx::default();
  drop_all_mesh_handles();
}

#[wasm_bindgen]
pub fn geoscript_repl_get_err(ctx: *mut GeoscriptReplCtx) -> String {
  let ctx = unsafe { &mut *ctx };
  match &ctx.last_result {
    Ok(_) => String::new(),
    Err(err) => format!("{err}"),
  }
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.output_meshes.len()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_transform(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh.transform.unwrap().as_slice().to_owned()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_vertices(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh.vertices.clone()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_indices(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Vec<usize> {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh.indices.clone()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_normals(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Option<Vec<f32>> {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh.shading_normals.as_ref().map(|normals| normals.clone())
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_path_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_paths.len()
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_path(ctx: *const GeoscriptReplCtx, path_ix: usize) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let path = { ctx.geo_ctx.rendered_paths.inner.borrow()[path_ix].clone() };
  let raw_path: Vec<f32> =
    unsafe { std::slice::from_raw_parts(path.as_ptr() as *const f32, path.len() * 3).to_vec() };
  std::mem::forget(path);
  raw_path
}

// TODO: in a perfect world, this would live in a dedicated tiny lightweight wasm module, but I
// don't care
#[wasm_bindgen]
pub fn geoscript_repl_get_serialized_builtin_fn_defs() -> String {
  maybe_init();
  geoscript::get_serialized_builtin_fn_defs()
}
