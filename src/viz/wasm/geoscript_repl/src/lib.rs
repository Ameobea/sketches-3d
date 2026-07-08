use std::rc::Rc;

use fxhash::FxHashMap;
use geoscript::{
  eval_program_into_scope,
  materials::Material,
  optimizer::optimize_ast,
  parse_program_maybe_with_prelude, parse_program_src, traverse_fn_calls,
  value_json::{serialize_bindings_to_json, serialize_value_to_json},
  ErrorStack, EvalCtx, GizmoKind, Mat4, Program, Scope, Sym, Value, PRELUDE,
};
use mesh::{
  linked_mesh::{mesh_flags, Vec3},
  OwnedIndexedMesh,
};
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
    std::mem::size_of::<Value>(),
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
  /// Root program's own top-level scope after the last successful eval, retained so
  /// `geotoy eval` can read its exports and evaluate follow-up expressions against it.
  pub last_root_scope: Option<Scope>,
  /// Value of the last top-level statement of the last successful eval.
  pub last_value: Option<Value>,
}

impl Default for GeoscriptReplCtx {
  fn default() -> Self {
    Self {
      geo_ctx: EvalCtx::default().set_log_fn(log),
      last_program: Err(ErrorStack::new("No program parsed yet")),
      last_result: Ok(()),
      output_meshes: Vec::new(),
      last_root_scope: None,
      last_value: None,
    }
  }
}

impl GeoscriptReplCtx {
  pub fn convert_rendered_meshes(&mut self) {
    self.output_meshes.clear();

    for rendered in self.geo_ctx.rendered_meshes.inner.borrow_mut().drain(..) {
      let mesh_handle = rendered.mesh;
      let mut mesh = (*mesh_handle.mesh).clone();

      // Weld and normal-recompute are decided independently. A complete set of shading normals
      // means the mesh authored its own — skip the auto-smooth recompute. Welding can't be
      // inferred from normals: a mesh with attribute seams (UV cuts, duplicated rings) has
      // position-coincident verts that must stay distinct, so `rail_sweep`/`compute_uvs` set
      // `NO_WELD`. A complete-normal mesh also skips welding for back-compat (it never
      // re-welds an authored mesh).
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
        // `mesh` is a throwaway clone, so the consuming finalize's inconsistent topology never
        // escapes.
        mesh.separate_normals_and_finalize(true, false, false)
      } else {
        mesh.to_raw_indexed(true, false, false)
      };
      owned_mesh.transform = Some(mesh_handle.transform);
      self.output_meshes.push(OutputMesh {
        mesh: owned_mesh,
        material: match &mesh_handle.material {
          Some(mat) => match &**mat {
            Material::External(name) => Some(name.clone()),
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
  ctx.geo_ctx.prints.borrow_mut().clear();
  ctx.last_root_scope = None;
  ctx.last_value = None;
  // The entry-point program is `_root`'s emitted source; tag its renders accordingly
  // so JS-side ancestor-transform composition can find the source node.
  let prev_module = ctx
    .geo_ctx
    .current_module
    .borrow_mut()
    .replace("_root".to_owned());
  let root_scope = ctx.geo_ctx.root_program_scope();
  ctx.last_result = match eval_program_into_scope(&ctx.geo_ctx, program, &root_scope) {
    Ok(val) => {
      ctx.last_value = Some(val);
      ctx.last_root_scope = Some(root_scope);
      Ok(())
    }
    Err(err) => Err(err),
  };
  *ctx.geo_ctx.current_module.borrow_mut() = prev_module;
  ctx.convert_rendered_meshes();
}

/// Root program's own top-level bindings from the last successful eval, as a JSON object
/// of tagged values keyed by name. Names shared with the ambient (prelude/globals) scope
/// are excluded so only the composition's own definitions are returned.
#[wasm_bindgen]
pub fn geoscript_repl_get_exports_json(ctx: *mut GeoscriptReplCtx, sample_count: u32) -> String {
  let ctx = unsafe { &*ctx };
  let Some(scope) = ctx.last_root_scope.as_ref() else {
    return "{}".to_owned();
  };
  let ambient_keys = ctx
    .geo_ctx
    .ambient_scope
    .borrow()
    .as_ref()
    .map(|s| s.own_keys())
    .unwrap_or_default();
  let bindings: Vec<(String, Value)> = scope
    .own_bindings()
    .into_iter()
    .filter(|(sym, _)| !ambient_keys.contains(sym))
    .map(|(sym, val)| (ctx.geo_ctx.with_resolved_sym(sym, |s| s.to_owned()), val))
    .collect();
  serialize_bindings_to_json(&ctx.geo_ctx, &bindings, sample_count as usize)
}

/// Serialize the value of the last top-level statement of the last successful eval as a
/// tagged value. `geotoy eval --expr` appends the expression as that final statement, so this
/// returns its value — fully resolved/optimized because it ran as part of the program.
#[wasm_bindgen]
pub fn geoscript_repl_get_last_value_json(ctx: *mut GeoscriptReplCtx, sample_count: u32) -> String {
  let ctx = unsafe { &*ctx };
  match &ctx.last_value {
    Some(val) => serialize_value_to_json(&ctx.geo_ctx, val, sample_count as usize),
    None => "{\"t\":\"nil\"}".to_owned(),
  }
}

/// Drain the `print()` output captured during the last eval.
#[wasm_bindgen]
pub fn geoscript_repl_take_prints(ctx: *mut GeoscriptReplCtx) -> Vec<String> {
  let ctx = unsafe { &mut *ctx };
  std::mem::take(&mut *ctx.geo_ctx.prints.borrow_mut())
}

#[wasm_bindgen]
pub fn geoscript_repl_get_used_async_deps(_ctx: *const GeoscriptReplCtx) -> u32 {
  #[cfg(target_arch = "wasm32")]
  {
    geoscript::get_async_dep_bits()
  }
  #[cfg(not(target_arch = "wasm32"))]
  {
    0
  }
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
  ctx.last_root_scope = None;
  ctx.last_value = None;
  ctx.geo_ctx.prints.borrow_mut().clear();

  ctx.geo_ctx.rendered_meshes.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_lights.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_paths.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_gizmos.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_controls.inner.borrow_mut().clear();

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
    new_hashes.insert(name.clone(), EvalCtx::compute_source_hash(source));
  }

  {
    let mut exports = ctx.geo_ctx.module_exports.borrow_mut();
    let mut lru = ctx.geo_ctx.module_exports_lru.borrow_mut();
    exports.retain(|name, entry| {
      new_hashes
        .get(name)
        .map(|h| *h == entry.source_hash)
        .unwrap_or(false)
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
  let new_hash = EvalCtx::compute_source_hash(&combined);
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
  ctx.geo_ctx.rendered_controls.inner.borrow_mut().clear();
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
          Material::External(name) => {
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
pub fn geoscript_repl_get_rendered_mesh_id(ctx: *const GeoscriptReplCtx, mesh_ix: usize) -> u32 {
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
        Material::External(name) => name.clone(),
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
  let path = {
    ctx.geo_ctx.rendered_paths.inner.borrow()[path_ix]
      .points
      .clone()
  };
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

/// One host-injected handle value (gizmo or control). `value` carries the numeric
/// payload — 3 floats for `vec3`/`color`, 16 for `transform`, 1 for `float`/`int`/`bool`;
/// `str_value` carries the `string`/`select` payload.
#[derive(DeJson)]
struct GizmoValueWire {
  kind: String,
  #[nserde(default)]
  value: Vec<f32>,
  str_value: Option<String>,
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
  let mut map: FxHashMap<String, FxHashMap<String, Value>> = FxHashMap::default();
  for ((module, handle), vjson) in module_names
    .iter()
    .zip(handle_ids.iter())
    .zip(values_json.iter())
  {
    let Ok(wire) = GizmoValueWire::deserialize_json(vjson) else {
      continue;
    };
    let value = match wire.kind.as_str() {
      "vec3" | "color" if wire.value.len() >= 3 => {
        Value::Vec3(Vec3::new(wire.value[0], wire.value[1], wire.value[2]))
      }
      "transform" if wire.value.len() >= 16 => {
        Value::Mat4(Rc::new(Mat4::from_column_slice(&wire.value[..16])))
      }
      "float" if !wire.value.is_empty() => Value::Float(wire.value[0]),
      "int" if !wire.value.is_empty() => Value::Int(wire.value[0] as i64),
      "bool" if !wire.value.is_empty() => Value::Bool(wire.value[0] != 0.),
      // Spline: flat 3·N floats → eager sequence of vec3 points.
      "spline" => geoscript::eager_seq_value(
        wire
          .value
          .chunks_exact(3)
          .map(|c| Value::Vec3(Vec3::new(c[0], c[1], c[2])))
          .collect(),
      ),
      "string" | "select" => match wire.str_value {
        Some(s) => Value::String(s),
        None => continue,
      },
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
    GizmoKind::Vec3 => {
      let v = match &g.current_value {
        Value::Vec3(v) => vec![v.x, v.y, v.z],
        _ => vec![0., 0., 0.],
      };
      ("vec3".to_owned(), v)
    }
    GizmoKind::Transform => {
      let v = match &g.current_value {
        Value::Mat4(m) => m.as_slice().to_vec(),
        _ => Vec::new(),
      };
      ("transform".to_owned(), v)
    }
  };
  RenderedGizmoWire {
    source_module: g.source_module.clone(),
    handle_id: g.handle_id.clone(),
    kind,
    origin: vec![
      g.resolved_origin.x,
      g.resolved_origin.y,
      g.resolved_origin.z,
    ],
    value,
    absolute: g.absolute,
    axes: g.axes.to_vec(),
    ghost: g.ghost,
  }
  .serialize_json()
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_control_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_controls.len()
}

#[derive(SerJson)]
struct RenderedControlWire {
  source_module: Option<String>,
  handle_id: String,
  kind: String,
  label: Option<String>,
  value: Vec<f32>,
  str_value: Option<String>,
  min: Option<f64>,
  max: Option<f64>,
  step: Option<f64>,
  style: Option<String>,
  options: Vec<String>,
}

#[wasm_bindgen]
pub fn geoscript_repl_get_rendered_control(
  ctx: *const GeoscriptReplCtx,
  control_ix: usize,
) -> String {
  let ctx = unsafe { &*ctx };
  let controls = ctx.geo_ctx.rendered_controls.inner.borrow();
  let c = &controls[control_ix];
  let kind = match c.kind {
    geoscript::ControlKind::Float => "float",
    geoscript::ControlKind::Int => "int",
    geoscript::ControlKind::Bool => "bool",
    geoscript::ControlKind::Color => "color",
    geoscript::ControlKind::Select => "select",
    geoscript::ControlKind::Spline => "spline",
  }
  .to_owned();
  let (value, str_value): (Vec<f32>, Option<String>) = match &c.current_value {
    Value::Float(f) => (vec![*f], None),
    Value::Int(i) => (vec![*i as f32], None),
    Value::Bool(b) => (vec![if *b { 1. } else { 0. }], None),
    Value::Vec3(v) => (vec![v.x, v.y, v.z], None),
    Value::String(s) => (Vec::new(), Some(s.clone())),
    // Spline: eager sequence of vec3 → flat 3·N floats.
    Value::Sequence(seq) => {
      let mut flat = Vec::new();
      for item in seq.consume(&ctx.geo_ctx) {
        if let Ok(Value::Vec3(v)) = item {
          flat.extend_from_slice(&[v.x, v.y, v.z]);
        }
      }
      (flat, None)
    }
    _ => (Vec::new(), None),
  };
  RenderedControlWire {
    source_module: c.source_module.clone(),
    handle_id: c.handle_id.clone(),
    kind,
    label: c.label.clone(),
    value,
    str_value,
    min: c.min,
    max: c.max,
    step: c.step,
    style: c.style.clone(),
    options: c.options.clone(),
  }
  .serialize_json()
}

#[wasm_bindgen]
pub fn geoscript_set_default_material(ctx: *mut GeoscriptReplCtx, material_name: Option<String>) {
  let ctx = unsafe { &mut *ctx };
  ctx
    .geo_ctx
    .default_material
    .replace(material_name.map(|material_name| Rc::new(Material::External(material_name))));
}

#[wasm_bindgen]
pub fn geoscript_set_materials(
  ctx: *mut GeoscriptReplCtx,
  materials: Vec<String>,
) -> Result<(), String> {
  let ctx = unsafe { &mut *ctx };
  let mut new_materials: FxHashMap<String, Rc<Material>> = FxHashMap::default();
  for material in materials {
    new_materials.insert(material.clone(), Rc::new(Material::External(material)));
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

  /// Flat quad split into two tris that DUPLICATE the shared diagonal edge — 6 verts at 4
  /// positions, i.e. a UV-seam-like coincident-vertex pair that distance-welding would collapse.
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

    // `NO_WELD` keeps the seam duplicates distinct — normals are still recomputed (the mesh
    // authored none), proving the two decisions are independent.
    let mut seamed = seam_quad();
    seamed.flags |= mesh_flags::NO_WELD;
    assert_eq!(finalized_vertex_count(seamed), 6);
  }
}
