use std::rc::Rc;

use fxhash::FxHashMap;
use geoscript::{
  eval_program_with_ctx, materials::Material, mesh_ops::mesh_boolean::drop_all_mesh_handles,
  optimize_ast, parse_program_maybe_with_prelude, traverse_fn_calls, ErrorStack, EvalCtx, Program,
};
use mesh::OwnedIndexedMesh;
use nanoserde::SerJson;
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

  assert_eq!(
    std::mem::size_of::<geoscript::Value>(),
    16,
    "would like to keep this 16 bytes"
  );
  console_error_panic_hook::set_once();
  wasm_logger::init(wasm_logger::Config::new(log::Level::Info));
}

pub struct OutputMesh {
  pub mesh: OwnedIndexedMesh,
  pub material: Option<String>,
}

pub struct GeoscriptReplCtx {
  pub geo_ctx: EvalCtx,
  pub last_program: Result<Program, ErrorStack>,
  pub last_result: Result<(), ErrorStack>,
  pub output_meshes: Vec<OutputMesh>,
}

impl Default for GeoscriptReplCtx {
  fn default() -> Self {
    Self {
      geo_ctx: EvalCtx::default().set_log_fn(log),
      last_program: Err(ErrorStack::new("No program parsed yet")),
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
      self.output_meshes.push(OutputMesh {
        mesh: owned_mesh,
        material: match &mesh_handle.material {
          Some(mat) => match &**mat {
            geoscript::materials::Material::External(name) => Some(name.clone()),
          },
          None => None,
        },
      });
    }
  }
}

#[wasm_bindgen]
pub fn geoscript_repl_init() -> *mut GeoscriptReplCtx {
  maybe_init();

  Box::into_raw(Box::new(GeoscriptReplCtx::default()))
}

#[wasm_bindgen]
pub fn geoscript_repl_parse_program(
  ctx: *mut GeoscriptReplCtx,
  src: String,
  include_prelude: bool,
) {
  let ctx = unsafe { &mut *ctx };
  ctx.last_program = parse_program_maybe_with_prelude(src, include_prelude);
  ctx.last_result = Ok(());
}

#[derive(Default, SerJson)]
pub struct GeoscriptAsyncDependencies {
  pub geodesics: bool,
}

#[wasm_bindgen]
pub fn geoscript_repl_get_async_dependencies(ctx: *mut GeoscriptReplCtx) -> String {
  let ctx = unsafe { &mut *ctx };
  let Ok(program) = &ctx.last_program else {
    panic!("This should not be called if parsing the program resulted in an error");
  };

  let mut deps = GeoscriptAsyncDependencies::default();
  traverse_fn_calls(program, |name: &'_ str| {
    if name == "trace_geodesic_path" {
      deps.geodesics = true;
    }
  });

  deps.serialize_json()
}

#[wasm_bindgen]
pub fn geoscript_repl_eval(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };
  let Ok(program) = &mut ctx.last_program else {
    ctx.last_result = Err(ErrorStack::new(
      "This should not be called if parsing the program resulted in an error",
    ));
    return;
  };
  if let Err(err) = optimize_ast(&ctx.geo_ctx, program) {
    ctx.last_result = Err(err);
    return;
  }
  ctx.last_result = eval_program_with_ctx(&ctx.geo_ctx, program);
  ctx.convert_rendered_meshes();
}

#[wasm_bindgen]
pub fn geoscript_repl_reset(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };
  let materials = std::mem::take(&mut ctx.geo_ctx.materials);
  let textures = std::mem::take(&mut ctx.geo_ctx.textures);
  let default_material = std::mem::take(&mut ctx.geo_ctx.default_material);
  *ctx = GeoscriptReplCtx::default();
  ctx.geo_ctx.materials = materials;
  ctx.geo_ctx.textures = textures;
  ctx.geo_ctx.default_material = default_material;
  drop_all_mesh_handles();
}

#[wasm_bindgen]
pub fn geoscript_repl_get_err(ctx: *mut GeoscriptReplCtx) -> String {
  let ctx = unsafe { &mut *ctx };

  if let Err(err) = &ctx.last_program {
    return format!("{err}");
  }

  match &ctx.last_result {
    Ok(_) => String::new(),
    Err(err) => format!("{err}"),
  }
}

#[wasm_bindgen]
pub fn geoscript_repl_has_err(ctx: *mut GeoscriptReplCtx) -> bool {
  let ctx = unsafe { &mut *ctx };

  if let Err(_) = &ctx.last_program {
    return true;
  }

  match &ctx.last_result {
    Ok(_) => false,
    Err(_) => true,
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
  mesh.mesh.transform.unwrap().as_slice().to_owned()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_vertices(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh.mesh.vertices.clone()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_indices(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Vec<usize> {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh.mesh.indices.clone()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_normals(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Option<Vec<f32>> {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh
    .mesh
    .shading_normals
    .as_ref()
    .map(|normals| normals.clone())
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_material(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> String {
  let ctx = unsafe { &*ctx };
  let mesh = &ctx.output_meshes[mesh_ix];
  mesh
    .material
    .clone()
    .unwrap_or_else(|| match &*ctx.geo_ctx.default_material.borrow() {
      Some(mat) => match &**mat {
        geoscript::materials::Material::External(name) => name.clone(),
      },
      None => String::new(),
    })
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

#[wasm_bindgen]
pub fn geoscript_get_rendered_light_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_lights.len()
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_light(ctx: *const GeoscriptReplCtx, light_ix: usize) -> String {
  let ctx = unsafe { &*ctx };
  let light = &ctx.geo_ctx.rendered_lights.inner.borrow()[light_ix];
  SerJson::serialize_json(light)
}

#[wasm_bindgen]
pub fn geoscript_set_default_material(ctx: *mut GeoscriptReplCtx, material_name: Option<String>) {
  let ctx = unsafe { &mut *ctx };
  ctx.geo_ctx.default_material.replace(
    material_name
      .map(|material_name| Rc::new(geoscript::materials::Material::External(material_name))),
  );
}

#[wasm_bindgen]
pub fn geoscript_set_materials(
  ctx: *mut GeoscriptReplCtx,
  materials: Vec<String>,
) -> Result<(), String> {
  let ctx = unsafe { &mut *ctx };
  let mut new_materials: FxHashMap<String, Rc<Material>> = FxHashMap::default();
  for material in materials {
    new_materials.insert(
      material.clone(),
      Rc::new(geoscript::materials::Material::External(material)),
    );
  }
  ctx.geo_ctx.materials = new_materials;
  if ctx.geo_ctx.materials.len() == 1 {
    ctx
      .geo_ctx
      .default_material
      .replace(ctx.geo_ctx.materials.values().next().cloned());
  }
  Ok(())
}

// TODO: in a perfect world, this would live in a dedicated tiny lightweight wasm module, but I
// don't care
#[wasm_bindgen]
pub fn geoscript_repl_get_serialized_builtin_fn_defs() -> String {
  maybe_init();
  geoscript::get_serialized_builtin_fn_defs()
}
