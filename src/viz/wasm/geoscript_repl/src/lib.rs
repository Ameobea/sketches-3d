use std::rc::Rc;

use fxhash::FxHashMap;
use geoscript::{
  eval_program_with_ctx, materials::Material, optimizer::optimize_ast,
  parse_program_maybe_with_prelude, parse_program_src, traverse_fn_calls, ErrorStack, EvalCtx,
  Program, Scope, Sym, PRELUDE,
};
use mesh::{linked_mesh::mesh_flags, OwnedIndexedMesh};
use nanoserde::{DeJson, SerJson};
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
  /// Module that called `render()`; JS composes tree-transforms from this. `None`
  /// for renders fired outside any module (e.g. ambient construction) — JS drops those.
  pub source_module: Option<String>,
  pub mesh_id: u32,
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

    for rendered in self.geo_ctx.rendered_meshes.inner.borrow_mut().drain(..) {
      let mesh_handle = rendered.mesh;
      let mut mesh = (*mesh_handle.mesh).clone();

      // Weld and normal-recompute are decided independently. A complete set of shading normals means
      // the mesh authored its own — skip the auto-smooth recompute. Welding can't be inferred from
      // normals: a mesh with attribute seams (UV cuts, duplicated rings) has position-coincident
      // verts that must stay distinct, so `rail_sweep`/`compute_uvs` set `NO_WELD`. A complete-normal
      // mesh also skips welding for back-compat (it never re-welds an authored mesh).
      let complete_normals =
        !mesh.shading_normals.is_empty() && mesh.shading_normals.len() == mesh.vertices.len();
      let skip_weld = mesh.has_flag(mesh_flags::NO_WELD) || complete_normals;

      if !skip_weld {
        let merged_count = mesh.merge_vertices_by_distance(0.0001);
        if merged_count > 0 {
          ::log::info!("Merged {merged_count} vertices in mesh");
        }
      }
      let mut owned_mesh = if !complete_normals {
        mesh.mark_edge_sharpness(
          self
            .geo_ctx
            .sharp_angle_threshold_degrees
            .borrow()
            .to_radians(),
        );
        // `mesh` is a throwaway clone, so the consuming finalize's inconsistent topology never escapes.
        mesh.separate_normals_and_finalize(true, false, false)
      } else {
        mesh.to_raw_indexed(true, false, false)
      };
      owned_mesh.transform = Some(mesh_handle.transform);
      self.output_meshes.push(OutputMesh {
        mesh: owned_mesh,
        material: match &mesh_handle.material {
          Some(mat) => match &**mat {
            geoscript::materials::Material::External(name) => Some(name.clone()),
          },
          None => None,
        },
        source_module: rendered.source_module,
        mesh_id: rendered.mesh_id,
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
  ctx.last_program = parse_program_maybe_with_prelude(&ctx.geo_ctx, src, include_prelude);
  ctx.last_result = Ok(());
}

#[derive(Default, SerJson)]
pub struct GeoscriptAsyncDependencies {
  pub geodesics: bool,
  pub cgal: bool,
  pub clipper2: bool,
  pub uv_unwrap: bool,
}

#[wasm_bindgen]
pub fn geoscript_repl_get_async_dependencies(ctx: *mut GeoscriptReplCtx) -> String {
  let ctx = unsafe { &mut *ctx };
  let Ok(program) = &ctx.last_program else {
    panic!("This should not be called if parsing the program resulted in an error");
  };

  let mut deps = GeoscriptAsyncDependencies::default();
  let check_dep = |name: Sym, deps: &mut GeoscriptAsyncDependencies| {
    ctx.geo_ctx.with_resolved_sym(name, |name| {
      if name == "trace_geodesic_path" {
        deps.geodesics = true;
      } else if name == "offset_path" {
        deps.clipper2 = true;
      } else if name == "compute_uvs" {
        deps.uv_unwrap = true;
      } else if name == "alpha_wrap"
        || name == "smooth"
        || name == "remesh_planar_patches"
        || name == "isotropic_remesh"
        || name == "remesh"
        || name == "remesh_isotropic"
        || name == "delaunay_remesh"
        || name == "remesh_delaunay"
      {
        deps.cgal = true;
      }
    })
  };

  traverse_fn_calls(program, |name: Sym| check_dep(name, &mut deps));

  // Also scan all registered module sources for async deps
  for source in ctx.geo_ctx.module_sources.borrow().values() {
    if let Ok(module_ast) = parse_program_src(&ctx.geo_ctx, source) {
      traverse_fn_calls(&module_ast, |name: Sym| check_dep(name, &mut deps));
    }
  }

  deps.serialize_json()
}

#[wasm_bindgen]
pub fn geoscript_repl_eval(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };
  #[cfg(target_arch = "wasm32")]
  geoscript::reset_async_dep_bits();
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
  // The entry-point program is `_root`'s emitted source; tag its renders accordingly
  // so JS-side ancestor-transform composition can find the source node.
  let prev_module = ctx
    .geo_ctx
    .current_module
    .borrow_mut()
    .replace("_root".to_owned());
  ctx.last_result = eval_program_with_ctx(&ctx.geo_ctx, program);
  *ctx.geo_ctx.current_module.borrow_mut() = prev_module;
  ctx.convert_rendered_meshes();
}

#[wasm_bindgen]
pub fn geoscript_repl_get_used_async_deps(_ctx: *const GeoscriptReplCtx) -> u32 {
  #[cfg(target_arch = "wasm32")]
  { geoscript::get_async_dep_bits() }
  #[cfg(not(target_arch = "wasm32"))]
  { 0 }
}

#[wasm_bindgen]
pub fn geoscript_repl_clear_const_eval_cache(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };
  ctx.geo_ctx.const_eval_cache.borrow_mut().entries.clear();
}

/// Reset per-run state. Caches, source map, id counter, and the symbol interner
/// are intentionally left in place so the cross-run module-result cache (and
/// `const_eval_cache`) can do its job.
#[wasm_bindgen]
pub fn geoscript_repl_reset(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };

  ctx.last_program = Err(ErrorStack::new("No program parsed yet"));
  ctx.last_result = Ok(());
  ctx.output_meshes.clear();

  ctx.geo_ctx.rendered_meshes.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_lights.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_paths.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_gizmos.inner.borrow_mut().clear();

  // Eval-scoped trackers: clear in case the previous run was interrupted mid-eval.
  ctx.geo_ctx.modules_in_flight.borrow_mut().clear();
  *ctx.geo_ctx.current_module.borrow_mut() = None;
  *ctx.geo_ctx.current_module_exports.borrow_mut() = None;
  *ctx.geo_ctx.current_module_imports.borrow_mut() = None;
  *ctx.geo_ctx.current_module_gizmo_reads.borrow_mut() = None;
  ctx.geo_ctx.current_module_unnamed_gizmo_count.set(0);
  // Gizmo inputs are eval-scoped host state; the runner re-pushes them each run.
  ctx.geo_ctx.gizmo_values.borrow_mut().clear();
  ctx.geo_ctx.replayed_this_run.borrow_mut().clear();

  ctx.geo_ctx.globals = Scope::default_globals(&ctx.geo_ctx.interned_symbols);
  *ctx.geo_ctx.ambient_scope.borrow_mut() = None;

  ctx.geo_ctx.reset_rng_to_default();
  #[cfg(target_arch = "wasm32")]
  geoscript::reset_async_dep_bits();

  *ctx.geo_ctx.sharp_angle_threshold_degrees.borrow_mut() = 45.8366;
  *ctx.geo_ctx.default_curve_angle_degrees.borrow_mut() = 1.0;

  // TODO: drop `MeshHandle`s no longer referenced by either the const-eval
  // cache or the module-exports cache.
}

#[wasm_bindgen]
pub fn geoscript_repl_set_module_sources(
  ctx: *mut GeoscriptReplCtx,
  module_names: Vec<String>,
  module_sources: Vec<String>,
) {
  let ctx = unsafe { &mut *ctx };

  // Hash incoming sources and diff against last-call hashes; only entries whose
  // source actually changed (or were removed) get evicted from `module_exports`.
  let mut new_hashes: fxhash::FxHashMap<String, u64> = fxhash::FxHashMap::default();
  for (name, source) in module_names.iter().zip(module_sources.iter()) {
    new_hashes.insert(
      name.clone(),
      geoscript::EvalCtx::compute_source_hash(source),
    );
  }

  {
    let mut exports = ctx.geo_ctx.module_exports.borrow_mut();
    let mut lru = ctx.geo_ctx.module_exports_lru.borrow_mut();
    exports.retain(|name, entry| {
      new_hashes.get(name).map(|h| *h == entry.source_hash).unwrap_or(false)
    });
    lru.retain(|name| exports.contains_key(name));
  }

  *ctx.geo_ctx.module_source_hashes.borrow_mut() = new_hashes;

  let mut sources = ctx.geo_ctx.module_sources.borrow_mut();
  sources.clear();
  for (name, source) in module_names.into_iter().zip(module_sources.into_iter()) {
    sources.insert(name, source);
  }
}

/// Build the ambient scope by evaluating each source in order; each source sees the
/// scope accumulated from the previous as its base. Used to construct prelude + globals
/// into a single ambient scope cloned for each subsequent module evaluation.
///
/// Module sources must be registered via `set_module_sources` before calling this if any
/// of the provided sources `import` from them.
#[wasm_bindgen]
pub fn geoscript_repl_set_ambient_scope_from_sources(
  ctx: *mut GeoscriptReplCtx,
  sources: Vec<String>,
) -> Result<(), String> {
  let ctx = unsafe { &mut *ctx };

  // Cached evals were resolved against the previous ambient — invalidate iff
  // the joined sources actually changed.
  let combined: String = {
    let mut s = String::new();
    for src in &sources {
      s.push_str(src);
      s.push('\n');
    }
    s
  };
  let new_hash = geoscript::EvalCtx::compute_source_hash(&combined);
  let prev_hash = *ctx.geo_ctx.last_ambient_hash.borrow();

  ctx.geo_ctx.clear_ambient_scope();
  if prev_hash != Some(new_hash) {
    ctx.geo_ctx.invalidate_module_cache();
  }

  let mut scope = Scope::default_globals(&ctx.geo_ctx.interned_symbols);
  for source in sources {
    ctx.geo_ctx.set_ambient_scope(scope.clone());
    scope = ctx
      .geo_ctx
      .evaluate_module_to_scope(&source)
      .map_err(|err| format!("{err}"))?;
  }
  ctx.geo_ctx.set_ambient_scope(scope);

  // Renders fired inside prelude / `_globals` aren't part of the user-visible
  // composition; drop them so they don't leak into the next eval.
  ctx.geo_ctx.rendered_meshes.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_lights.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_paths.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_gizmos.inner.borrow_mut().clear();
  // Ambient discarded any replayed side effects; let them fire again in `_root`.
  ctx.geo_ctx.replayed_this_run.borrow_mut().clear();
  *ctx.geo_ctx.last_ambient_hash.borrow_mut() = Some(new_hash);

  Ok(())
}

#[wasm_bindgen]
pub fn geoscript_repl_clear_ambient_scope(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };
  ctx.geo_ctx.clear_ambient_scope();
  ctx.geo_ctx.invalidate_module_cache();
  ctx.geo_ctx.replayed_this_run.borrow_mut().clear();
  *ctx.geo_ctx.last_ambient_hash.borrow_mut() = None;
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
pub fn geoscript_repl_get_rendered_mesh_indices_with_material(
  ctx: *const GeoscriptReplCtx,
  mat_name: &str,
) -> Vec<usize> {
  let ctx = unsafe { &*ctx };
  ctx
    .output_meshes
    .iter()
    .enumerate()
    .filter_map(|(ix, mesh)| match &mesh.material {
      Some(name) => {
        if name == mat_name {
          Some(ix)
        } else {
          None
        }
      }
      None => match &*ctx.geo_ctx.default_material.borrow() {
        Some(mat) => match &**mat {
          geoscript::materials::Material::External(name) => {
            if name == mat_name {
              Some(ix)
            } else {
              None
            }
          }
        },
        None => None,
      },
    })
    .collect()
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
pub fn geoscript_repl_get_rendered_mesh_uvs(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Option<Vec<f32>> {
  let ctx = unsafe { &*ctx };
  ctx.output_meshes[mesh_ix].mesh.uv.clone()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_tangents(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> Option<Vec<f32>> {
  let ctx = unsafe { &*ctx };
  ctx.output_meshes[mesh_ix].mesh.tangent.clone()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_source_module(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> String {
  let ctx = unsafe { &*ctx };
  ctx.output_meshes[mesh_ix]
    .source_module
    .clone()
    .unwrap_or_default()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_mesh_id(
  ctx: *const GeoscriptReplCtx,
  mesh_ix: usize,
) -> u32 {
  let ctx = unsafe { &*ctx };
  ctx.output_meshes[mesh_ix].mesh_id
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
  let path = { ctx.geo_ctx.rendered_paths.inner.borrow()[path_ix].points.clone() };
  let raw_path: Vec<f32> =
    unsafe { std::slice::from_raw_parts(path.as_ptr() as *const f32, path.len() * 3).to_vec() };
  std::mem::forget(path);
  raw_path
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_path_id(ctx: *const GeoscriptReplCtx, path_ix: usize) -> u32 {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_paths.inner.borrow()[path_ix].path_id
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_path_source_module(
  ctx: *const GeoscriptReplCtx,
  path_ix: usize,
) -> String {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_paths.inner.borrow()[path_ix]
    .source_module
    .clone()
    .unwrap_or_default()
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_light_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_lights.len()
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_light(ctx: *const GeoscriptReplCtx, light_ix: usize) -> String {
  let ctx = unsafe { &*ctx };
  let light = &ctx.geo_ctx.rendered_lights.inner.borrow()[light_ix].light;
  SerJson::serialize_json(light)
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_light_id(ctx: *const GeoscriptReplCtx, light_ix: usize) -> u32 {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_lights.inner.borrow()[light_ix].light_id
}

/// One host-injected gizmo value. `value` is 3 floats for `vec3` or a 16-float
/// column-major matrix for `transform`.
#[derive(DeJson)]
struct GizmoValueWire {
  kind: String,
  value: Vec<f32>,
}

/// Replace the full gizmo-value map. Parallel arrays: the i-th value is keyed by
/// `module_names[i]` → `handle_ids[i]`. Called before `eval`, like `set_ambient_scope`.
#[wasm_bindgen]
pub fn geoscript_repl_set_gizmo_values(
  ctx: *mut GeoscriptReplCtx,
  module_names: Vec<String>,
  handle_ids: Vec<String>,
  values_json: Vec<String>,
) {
  let ctx = unsafe { &mut *ctx };
  let mut map: FxHashMap<String, FxHashMap<String, geoscript::Value>> = FxHashMap::default();
  for ((module, handle), vjson) in module_names
    .iter()
    .zip(handle_ids.iter())
    .zip(values_json.iter())
  {
    let Ok(wire) = GizmoValueWire::deserialize_json(vjson) else {
      continue;
    };
    let value = match wire.kind.as_str() {
      "vec3" if wire.value.len() >= 3 => geoscript::Value::Vec3(mesh::linked_mesh::Vec3::new(
        wire.value[0],
        wire.value[1],
        wire.value[2],
      )),
      "transform" if wire.value.len() >= 16 => {
        geoscript::Value::Mat4(Rc::new(geoscript::Mat4::from_column_slice(&wire.value[..16])))
      }
      _ => continue,
    };
    map
      .entry(module.clone())
      .or_default()
      .insert(handle.clone(), value);
  }
  *ctx.geo_ctx.gizmo_values.borrow_mut() = map;
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_gizmo_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_gizmos.len()
}

#[derive(SerJson)]
struct RenderedGizmoWire {
  source_module: Option<String>,
  handle_id: String,
  kind: String,
  origin: Vec<f32>,
  value: Vec<f32>,
  absolute: bool,
  axes: Vec<bool>,
  ghost: Option<bool>,
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_gizmo(ctx: *const GeoscriptReplCtx, gizmo_ix: usize) -> String {
  let ctx = unsafe { &*ctx };
  let gizmos = ctx.geo_ctx.rendered_gizmos.inner.borrow();
  let g = &gizmos[gizmo_ix];
  let (kind, value) = match g.kind {
    geoscript::GizmoKind::Vec3 => {
      let v = match &g.current_value {
        geoscript::Value::Vec3(v) => vec![v.x, v.y, v.z],
        _ => vec![0., 0., 0.],
      };
      ("vec3".to_owned(), v)
    }
    geoscript::GizmoKind::Transform => {
      let v = match &g.current_value {
        geoscript::Value::Mat4(m) => m.as_slice().to_vec(),
        _ => Vec::new(),
      };
      ("transform".to_owned(), v)
    }
  };
  RenderedGizmoWire {
    source_module: g.source_module.clone(),
    handle_id: g.handle_id.clone(),
    kind,
    origin: vec![g.resolved_origin.x, g.resolved_origin.y, g.resolved_origin.z],
    value,
    absolute: g.absolute,
    axes: g.axes.to_vec(),
    ghost: g.ghost,
  }
  .serialize_json()
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
  let materials_changed = ctx.geo_ctx.materials.len() != new_materials.len()
    || new_materials
      .keys()
      .any(|material| !ctx.geo_ctx.materials.contains_key(material));
  if materials_changed {
    ctx.geo_ctx.invalidate_module_cache();
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

#[wasm_bindgen]
pub fn geoscript_repl_get_prelude() -> String {
  PRELUDE.to_owned()
}

// TODO: in a perfect world, this would live in a dedicated tiny lightweight wasm module, but I
// don't care
#[wasm_bindgen]
pub fn geoscript_repl_get_serialized_builtin_fn_defs() -> String {
  maybe_init();
  geoscript::get_serialized_builtin_fn_defs()
}

#[cfg(test)]
mod tests {
  use std::cell::RefCell;

  use geoscript::{ManifoldHandle, Mat4, MeshHandle, RenderedMesh};
  use mesh::{
    linked_mesh::{mesh_flags, Vec3},
    LinkedMesh,
  };

  use super::*;

  /// Flat quad split into two tris that DUPLICATE the shared diagonal edge — 6 verts at 4 positions,
  /// i.e. a UV-seam-like coincident-vertex pair that distance-welding would collapse.
  fn seam_quad() -> LinkedMesh<()> {
    let verts = [
      Vec3::new(0., 0., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(0., 0., 1.),
      Vec3::new(1., 0., 0.),
      Vec3::new(1., 0., 1.),
      Vec3::new(0., 0., 1.),
    ];
    LinkedMesh::from_indexed_vertices(&verts, &[0, 1, 2, 3, 4, 5], None, None)
  }

  fn finalized_vertex_count(mesh: LinkedMesh<()>) -> usize {
    let mut ctx = GeoscriptReplCtx::default();
    ctx.geo_ctx.rendered_meshes.push(RenderedMesh {
      mesh: Rc::new(MeshHandle {
        mesh: Rc::new(mesh),
        transform: Mat4::identity(),
        manifold_handle: Rc::new(ManifoldHandle::new(0)),
        aabb: RefCell::new(None),
        trimesh: RefCell::new(None),
        material: None,
      }),
      source_module: None,
      mesh_id: 0,
    });
    ctx.convert_rendered_meshes();
    ctx.output_meshes[0].mesh.vertices.len() / 3
  }

  #[test]
  fn no_weld_flag_decouples_welding_from_normal_recompute() {
    // Default finalize welds the coincident diagonal verts (6 -> 4) while recomputing normals.
    assert_eq!(finalized_vertex_count(seam_quad()), 4);

    // `NO_WELD` keeps the seam duplicates distinct — normals are still recomputed (the mesh authored
    // none), proving the two decisions are independent.
    let mut seamed = seam_quad();
    seamed.flags |= mesh_flags::NO_WELD;
    assert_eq!(finalized_vertex_count(seamed), 6);
  }
}
