use geoscript::{parse_and_eval_program_with_ctx, EvalCtx};
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
  pub last_result: Result<(), String>,
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
    // TODO: what's another clone lol
    for mut mesh in self.geo_ctx.rendered_meshes.meshes.lock().unwrap().clone() {
      mesh.mark_edge_sharpness(0.8);
      mesh.separate_vertices_and_compute_normals();

      let owned_mesh = mesh.to_raw_indexed(true, false, false);
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
}

#[wasm_bindgen]
pub fn geoscript_repl_get_err(ctx: *mut GeoscriptReplCtx) -> String {
  let ctx = unsafe { &mut *ctx };
  match &ctx.last_result {
    Ok(_) => String::new(),
    Err(err) => err.clone(),
  }
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.output_meshes.len()
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
