use std::rc::Rc;

use fxhash::FxHashMap;
use geoscript::{
  eval_resolved_program,
  materials::Material,
  optimizer::optimize_ast,
  parse_program_src, parse_program_with_prefix, prelude_for_kind, traverse_fn_calls,
  value_json::{serialize_bindings_to_json, serialize_value_to_json},
  ErrorStack, EvalCtx, GizmoKind, InjectedTextureParams, Mat4, Program, Scope, Sym, TextureFilter,
  TextureFormat, TextureHandle, TextureWrap, Value,
};
use mesh::{
  linked_mesh::{mesh_flags, Vec3},
  OwnedIndexedMesh,
};
use nanoserde::{DeJson, SerJson};
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[global_allocator]
static ALLOC: geoscript::aligned_alloc::CacheAligned = geoscript::aligned_alloc::CacheAligned;

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
  /// Root program's own top-level bindings after the last successful eval (declaration
  /// order), retained so `geotoy eval` can read its exports.
  pub last_root_bindings: Option<Vec<(Sym, Value)>>,
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
      last_root_bindings: None,
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
/// `prelude_kind` names the tree kind whose prelude to prepend; `None` when the entry tab has
/// ejected it.
pub fn geoscript_repl_parse_program(
  ctx: *mut GeoscriptReplCtx,
  src: String,
  prelude_kind: Option<String>,
) {
  let ctx = unsafe { &mut *ctx };
  let prelude = prelude_kind.as_deref().map(prelude_for_kind).unwrap_or("");
  ctx.last_program = parse_program_with_prefix(&ctx.geo_ctx, src, prelude);
  ctx.last_result = Ok(());
}

#[derive(Default, SerJson)]
pub struct GeoscriptAsyncDependencies {
  pub geodesics: bool,
  pub cgal: bool,
  pub clipper2: bool,
  pub uv_unwrap: bool,
  pub uv_solvers: bool,
  pub model_data: bool,
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
        // `type` decides BFF vs the tube/strip solver module; preload both.
        deps.uv_unwrap = true;
        deps.uv_solvers = true;
      } else if name == "utah_teapot"
        || name == "teapot"
        || name == "stanford_bunny"
        || name == "bunny"
      {
        deps.model_data = true;
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

/// `root_module` names the entry program for module resolution and render attribution.
/// Hosts that qualify their module keys pass `<tabId>:_root` so the entry's own bare
/// imports resolve within that tab; omitting it keeps the unqualified `_root` default.
#[wasm_bindgen]
pub fn geoscript_repl_eval(ctx: *mut GeoscriptReplCtx, root_module: Option<String>) {
  let ctx = unsafe { &mut *ctx };
  #[cfg(target_arch = "wasm32")]
  geoscript::reset_async_dep_bits();
  let Ok(program) = &mut ctx.last_program else {
    ctx.last_result = Err(ErrorStack::new(
      "This should not be called if parsing the program resulted in an error",
    ));
    return;
  };
  // Static rejection beats the runtime in-flight guard: the error names the whole chain
  // and fires deterministically instead of at whichever import happens to run first.
  if let Some(chain) = ctx.geo_ctx.detect_import_cycle() {
    ctx.last_result = Err(ErrorStack::new(format!(
      "Circular import detected in module \"{}\": {}",
      chain[0],
      chain.join(" -> ")
    )));
    return;
  }
  // The entry-point program is `_root`'s emitted source; tag its renders accordingly
  // so JS-side ancestor-transform composition can find the source node. Set BEFORE
  // `optimize_ast` so the const-folder seeds from the entry tab's own ambient scope.
  let prev_module = ctx
    .geo_ctx
    .current_module
    .borrow_mut()
    .replace(root_module.unwrap_or_else(|| "_root".to_owned()));
  // Vectorize reports accumulate from const folding onward; body ids are per-parse, so
  // stale entries from earlier runs would otherwise pile up.
  ctx.geo_ctx.tex_vectorize.reports.borrow_mut().clear();
  if let Err(err) = optimize_ast(&ctx.geo_ctx, program) {
    *ctx.geo_ctx.current_module.borrow_mut() = prev_module;
    ctx.last_result = Err(err);
    return;
  }
  ctx.geo_ctx.prints.borrow_mut().clear();
  ctx.last_root_bindings = None;
  ctx.last_value = None;
  let ambient = ctx.geo_ctx.ambient_scope_for_current();
  let base = match &ambient {
    Some(scope) => &**scope,
    None => &ctx.geo_ctx.globals,
  };
  ctx.last_result = match eval_resolved_program(&ctx.geo_ctx, program, base) {
    Ok((val, bindings)) => {
      ctx.last_value = Some(val);
      ctx.last_root_bindings = Some(bindings);
      Ok(())
    }
    Err(err) => Err(err),
  };
  *ctx.geo_ctx.current_module.borrow_mut() = prev_module;
  ctx.geo_ctx.apply_injected_texture_params();
  ctx.convert_rendered_meshes();
}

/// Root program's own top-level bindings from the last successful eval, as a JSON object
/// of tagged values keyed by name in declaration order. These are exactly the program's own
/// definitions — ambient (prelude/globals) bindings are never included.
#[wasm_bindgen]
pub fn geoscript_repl_get_exports_json(ctx: *mut GeoscriptReplCtx, sample_count: u32) -> String {
  let ctx = unsafe { &*ctx };
  let Some(root_bindings) = ctx.last_root_bindings.as_ref() else {
    return "{}".to_owned();
  };
  let bindings: Vec<(String, Value)> = root_bindings
    .iter()
    .map(|(sym, val)| {
      (
        ctx.geo_ctx.with_resolved_sym(*sym, |s| s.to_owned()),
        val.clone(),
      )
    })
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

/// `[entries, retained_bytes, max_bytes]` for the cross-run const-eval cache. The cap is
/// reported alongside the occupancy so the host readout doesn't duplicate the constant.
#[wasm_bindgen]
pub fn geoscript_repl_get_const_eval_cache_stats(ctx: *const GeoscriptReplCtx) -> Vec<f64> {
  let ctx = unsafe { &*ctx };
  let cache = ctx.geo_ctx.const_eval_cache.borrow();
  vec![
    cache.len() as f64,
    cache.retained_bytes() as f64,
    cache.max_bytes() as f64,
  ]
}

#[wasm_bindgen]
pub fn geoscript_repl_clear_const_eval_cache(ctx: *mut GeoscriptReplCtx) {
  let ctx = unsafe { &mut *ctx };
  ctx.geo_ctx.const_eval_cache.borrow_mut().clear();
}

#[derive(SerJson)]
struct VectorizeReportJson {
  vectorized: bool,
  reason: Option<String>,
  line: u32,
  col: u32,
  module: Option<String>,
  plan: Option<String>,
}

/// Texel-closure vectorizer outcomes from the last run, as a JSON array of
/// `{vectorized, reason, line, col, module}` — the "did this closure vectorize, and if not,
/// why" signal (a silent bail is a ~60× per-texel cliff). `line`/`col` are within the
/// named module's registered source.
#[wasm_bindgen]
pub fn geoscript_repl_get_vectorize_reports(ctx: *const GeoscriptReplCtx) -> String {
  let ctx = unsafe { &*ctx };
  let reports = ctx.geo_ctx.tex_vectorize.reports.borrow();
  let mut entries: Vec<VectorizeReportJson> = reports
    .values()
    .map(|r| VectorizeReportJson {
      vectorized: r.vectorized,
      reason: r.reason.clone(),
      line: r.loc.0,
      col: r.loc.1,
      module: r.module.clone(),
      plan: r.plan.clone(),
    })
    .collect();
  entries.sort_unstable_by_key(|r| (r.module.clone(), r.line, r.col));
  SerJson::serialize_json(&entries)
}

/// Kill switch for in-browser A/B and bisecting.
#[wasm_bindgen]
pub fn geoscript_repl_set_no_vectorize(ctx: *const GeoscriptReplCtx, no_vectorize: bool) {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.tex_vectorize.no_vectorize.set(no_vectorize);
}

/// Runs both paths and asserts bit-equality on every texel body — the env var behind this
/// doesn't exist on wasm, so the host needs a setter to reach it at all.
#[wasm_bindgen]
pub fn geoscript_repl_set_verify(ctx: *const GeoscriptReplCtx, verify: bool) {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.tex_vectorize.verify.set(verify);
}

/// Attach each vectorized body's plan listing with per-step timings to its report.
#[wasm_bindgen]
pub fn geoscript_repl_set_vectorize_profile(ctx: *const GeoscriptReplCtx, profile: bool) {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.tex_vectorize.profile.set(profile);
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
  ctx.last_root_bindings = None;
  ctx.last_value = None;
  ctx.geo_ctx.prints.borrow_mut().clear();

  ctx.geo_ctx.rendered_meshes.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_lights.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_paths.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_textures.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_gizmos.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_controls.inner.borrow_mut().clear();

  // Eval-scoped trackers: clear in case the previous run was interrupted mid-eval.
  ctx.geo_ctx.modules_in_flight.borrow_mut().clear();
  *ctx.geo_ctx.current_module.borrow_mut() = None;
  *ctx.geo_ctx.current_module_exports.borrow_mut() = None;
  *ctx.geo_ctx.current_module_imports.borrow_mut() = None;
  *ctx.geo_ctx.current_module_gizmo_reads.borrow_mut() = None;
  ctx.geo_ctx.current_module_unnamed_gizmo_count.set(0);
  ctx.geo_ctx.current_module_read_settings.set(false);
  // Gizmo inputs are eval-scoped host state; the runner re-pushes them each run.
  ctx.geo_ctx.gizmo_values.borrow_mut().clear();
  ctx.geo_ctx.injected_texture_params.borrow_mut().clear();
  ctx.geo_ctx.replayed_this_run.borrow_mut().clear();

  ctx.geo_ctx.globals = Scope::default_globals(&ctx.geo_ctx.interned_symbols);
  *ctx.geo_ctx.ambient_scope.borrow_mut() = None;
  ctx.geo_ctx.clear_tab_ambient_scopes();

  ctx.geo_ctx.tex_vectorize.reset_per_run();

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
  root_module: Option<String>,
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

  // Ambient sources may `import` from registered modules, so they need the same module
  // identity the entry program gets — otherwise a bare import inside `_globals` resolves
  // unqualified and can't find a `<tabId>:`-keyed module.
  let prev_module = ctx
    .geo_ctx
    .current_module
    .borrow_mut()
    .replace(root_module.unwrap_or_else(|| "_root".to_owned()));

  let mut scope = Scope::default_globals(&ctx.geo_ctx.interned_symbols);
  let result: Result<(), String> = (|| {
    for source in sources {
      ctx.geo_ctx.set_ambient_scope(scope.clone());
      scope = ctx
        .geo_ctx
        .evaluate_module_to_scope(&source)
        .map_err(|err| format!("{err}"))?;
    }
    Ok(())
  })();
  *ctx.geo_ctx.current_module.borrow_mut() = prev_module;
  result?;
  ctx.geo_ctx.set_ambient_scope(scope);

  // Renders fired inside prelude / `_globals` aren't part of the user-visible
  // composition; drop them so they don't leak into the next eval.
  ctx.geo_ctx.rendered_meshes.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_lights.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_paths.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_textures.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_gizmos.inner.borrow_mut().clear();
  ctx.geo_ctx.rendered_controls.inner.borrow_mut().clear();
  // Ambient discarded any replayed side effects; let them fire again in `_root`.
  ctx.geo_ctx.replayed_this_run.borrow_mut().clear();
  *ctx.geo_ctx.last_ambient_hash.borrow_mut() = Some(new_hash);

  Ok(())
}

/// Per-tab ambient scopes for multi-tab runs: one (prelude kind, `_globals` source) triple
/// per run-set tab, **active tab last** — the RNG is left where the last tab's construction
/// ended, so the entry program continues the active tab's stream exactly as the single-scope
/// path did. Each tab's scope is built on a freshly-reset RNG and the post-construction
/// state is recorded for tab-root stream resets. A tab's cached modules (plus transitive
/// importers) are evicted only when that tab's own ambient content changed.
///
/// Module sources must be registered first, as with `set_ambient_scope_from_sources`.
#[wasm_bindgen]
pub fn geoscript_repl_set_tab_ambient_scopes(
  ctx: *mut GeoscriptReplCtx,
  tab_ids: Vec<String>,
  prelude_kinds: Vec<String>,
  globals_sources: Vec<String>,
) -> Result<(), String> {
  let ctx = unsafe { &mut *ctx };
  let geo = &ctx.geo_ctx;

  for i in 0..tab_ids.len() {
    let hash =
      EvalCtx::compute_source_hash(&format!("{}\u{0}{}", prelude_kinds[i], globals_sources[i]));
    if geo.note_tab_ambient_hash(&tab_ids[i], hash) {
      geo.evict_modules_with_prefix(&tab_ids[i]);
    }
  }

  let prev_single = geo.ambient_scope.borrow().clone();
  let result: Result<(), String> = (|| {
    for i in 0..tab_ids.len() {
      let tab_id = &tab_ids[i];
      geo.reset_rng_to_default();
      let prev_module = geo
        .current_module
        .borrow_mut()
        .replace(format!("{tab_id}:_root"));
      let mut scope = Scope::default_globals(&geo.interned_symbols);
      let build: Result<(), String> = (|| {
        for source in [prelude_for_kind(&prelude_kinds[i]), &globals_sources[i]] {
          if source.is_empty() {
            continue;
          }
          // Transient stacking base, same mechanism as the single-scope path: the second
          // source sees the first's bindings via `fresh_module_scope`.
          geo.set_ambient_scope(scope.clone());
          scope = geo
            .evaluate_module_to_scope(source)
            .map_err(|err| format!("{err}"))?;
        }
        Ok(())
      })();
      *geo.current_module.borrow_mut() = prev_module;
      build?;
      geo.install_tab_ambient(tab_id, scope);
    }
    Ok(())
  })();
  *geo.ambient_scope.borrow_mut() = prev_single;

  // Prelude/`_globals` renders aren't part of the user-visible composition; drop them and
  // let replayed side effects fire again during the eval proper.
  geo.rendered_meshes.inner.borrow_mut().clear();
  geo.rendered_lights.inner.borrow_mut().clear();
  geo.rendered_paths.inner.borrow_mut().clear();
  geo.rendered_textures.inner.borrow_mut().clear();
  geo.rendered_gizmos.inner.borrow_mut().clear();
  geo.rendered_controls.inner.borrow_mut().clear();
  geo.replayed_this_run.borrow_mut().clear();

  result
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
pub fn geoscript_get_rendered_texture_count(ctx: *const GeoscriptReplCtx) -> usize {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_textures.len()
}

/// `[width, height, channels]`
#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_dims(
  ctx: *const GeoscriptReplCtx,
  tex_ix: usize,
) -> Vec<usize> {
  let ctx = unsafe { &*ctx };
  let tex = &ctx.geo_ctx.rendered_textures.inner.borrow()[tex_ix].texture;
  vec![tex.width, tex.height, tex.channels]
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_name(ctx: *const GeoscriptReplCtx, tex_ix: usize) -> String {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_textures.inner.borrow()[tex_ix]
    .name
    .clone()
}

/// Empty string when no usage was declared.
#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_usage(ctx: *const GeoscriptReplCtx, tex_ix: usize) -> String {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_textures.inner.borrow()[tex_ix]
    .usage
    .map(|u| u.as_str().to_owned())
    .unwrap_or_default()
}

/// "repeat" | "clamp" | "mirror"
#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_wrap(ctx: *const GeoscriptReplCtx, tex_ix: usize) -> String {
  let ctx = unsafe { &*ctx };
  let wrap = ctx.geo_ctx.rendered_textures.inner.borrow()[tex_ix]
    .texture
    .wrap;
  match wrap {
    TextureWrap::Repeat => "repeat",
    TextureWrap::Clamp => "clamp",
    TextureWrap::Mirror => "mirror",
  }
  .to_owned()
}

/// Layer count; 1 for plain `render_texture` outputs, the slice count for stacks.
#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_layers(ctx: *const GeoscriptReplCtx, tex_ix: usize) -> usize {
  let ctx = unsafe { &*ctx };
  1 + ctx.geo_ctx.rendered_textures.inner.borrow()[tex_ix]
    .extra_slices
    .len()
}

/// All slices concatenated in layer order; len = width*height*channels*layers.
/// Borrows a texture's channel planes as slices (materializing a view if needed) for the
/// encoders, which all read SoA directly rather than through an interleaved staging copy.
fn with_planes<R>(tex: &TextureHandle, f: impl FnOnce(&[&[f32]]) -> R) -> R {
  let planes = tex.as_planes();
  let refs: Vec<&[f32]> = planes.iter().map(|p| p.as_slice()).collect();
  f(&refs)
}

/// Slice handles for an output, with the `rendered_textures` borrow already released. The
/// encoders below allocate the whole output buffer up front, and an allocation failure on
/// wasm aborts without unwinding — a borrow held across one stays taken for the life of the
/// instance and panics every subsequent `reset`, turning one oversized run into a dead tab.
fn texture_slices(ctx: &GeoscriptReplCtx, tex_ix: usize) -> Vec<Rc<TextureHandle>> {
  let textures = ctx.geo_ctx.rendered_textures.inner.borrow();
  let rt = &textures[tex_ix];
  std::iter::once(&rt.texture)
    .chain(rt.extra_slices.iter())
    .map(Rc::clone)
    .collect()
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_pixels(
  ctx: *const GeoscriptReplCtx,
  tex_ix: usize,
) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let slices = texture_slices(ctx, tex_ix);
  let tex = &slices[0];
  let layer_len = tex.width * tex.height * tex.channels;
  let mut out = vec![0f32; layer_len * slices.len()];
  for (i, slice) in slices.iter().enumerate() {
    with_planes(slice, |planes| {
      geoscript::texture_encode::interleave(planes, &mut out[i * layer_len..(i + 1) * layer_len])
    });
  }
  out
}

/// Per-channel value stats of the first slice: `[min, max, mean, std, nonfinite, q_0 … q_256]`
/// per channel, concatenated (see `TexStats::to_wire`).
#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_stats(
  ctx: *const GeoscriptReplCtx,
  tex_ix: usize,
) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  texture_slices(ctx, tex_ix)[0].stats().to_wire()
}

/// RGBA-expanded f32 pixels (all slices concatenated; gray → rgb, b zero-filled, a=1).
/// Fetched by the host only for 3-channel textures — the one channel count whose raw
/// pixels can't be uploaded to a GPU-mippable float format directly.
#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_pixels_rgba(
  ctx: *const GeoscriptReplCtx,
  tex_ix: usize,
) -> Vec<f32> {
  let ctx = unsafe { &*ctx };
  let slices = texture_slices(ctx, tex_ix);
  let tex = &slices[0];
  let layer_len = tex.width * tex.height * 4;
  let mut out = vec![0f32; layer_len * slices.len()];
  for (i, slice) in slices.iter().enumerate() {
    with_planes(slice, |planes| {
      geoscript::texture_encode::expand_rgba_f32(
        planes,
        &mut out[i * layer_len..(i + 1) * layer_len],
      )
    });
  }
  out
}

/// unorm8-encoded pixels (all slices concatenated) for the texture's materialization
/// format; empty when the resolved format is float (consumers upload the raw f32s). An
/// unset format defaults to rgba8, mirroring the JS-side `DEFAULT_FORMAT`.
#[wasm_bindgen]
pub fn geoscript_encode_rendered_texture_pixels(
  ctx: *const GeoscriptReplCtx,
  tex_ix: usize,
) -> Vec<u8> {
  let ctx = unsafe { &*ctx };
  let slices = texture_slices(ctx, tex_ix);
  let tex = &slices[0];
  let format = tex.format.unwrap_or(TextureFormat::Rgba8);
  let bpp = match format {
    TextureFormat::Rgba8 => 4,
    TextureFormat::R8 => 1,
    TextureFormat::Rg8 => 2,
    _ => return Vec::new(),
  };
  let layer_len = tex.width * tex.height * bpp;
  let mut out = vec![0u8; layer_len * slices.len()];
  for (i, slice) in slices.iter().enumerate() {
    with_planes(slice, |planes| {
      geoscript::texture_encode::encode_unorm8(
        planes,
        format,
        &mut out[i * layer_len..(i + 1) * layer_len],
      )
    });
  }
  out
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_source_module(
  ctx: *const GeoscriptReplCtx,
  tex_ix: usize,
) -> String {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_textures.inner.borrow()[tex_ix]
    .source_module
    .clone()
    .unwrap_or_default()
}

#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_id(ctx: *const GeoscriptReplCtx, tex_ix: usize) -> u32 {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_textures.inner.borrow()[tex_ix].texture_id
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

#[wasm_bindgen]
pub fn geoscript_get_rendered_light_source_module(
  ctx: *const GeoscriptReplCtx,
  light_ix: usize,
) -> String {
  let ctx = unsafe { &*ctx };
  ctx.geo_ctx.rendered_lights.inner.borrow()[light_ix]
    .source_module
    .clone()
    .unwrap_or_default()
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
  let mut map: FxHashMap<String, geoscript::ValueMap> = FxHashMap::default();
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
      "image_levels" => match geoscript::image_levels_value_from_wire(&wire.value) {
        Some(v) => v,
        None => continue,
      },
      "string" | "select" => match wire.str_value {
        Some(s) => Value::String(s),
        None => continue,
      },
      // Ramp controls carry their spec as JSON; the ramp value (incl. LUT bake) is built
      // here so the builtin just hands it back.
      "ramp" => match wire
        .str_value
        .as_deref()
        .map(|s| geoscript::ramp_value_from_wire_json(s, &ctx.geo_ctx))
      {
        Some(Ok(v)) => v,
        _ => continue,
      },
      "uv_params" => match wire
        .str_value
        .as_deref()
        .map(geoscript::uv_params_value_from_wire_json)
      {
        Some(Ok(v)) => v,
        _ => continue,
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

/// Replace the full per-output texture GPU param map. Parallel arrays: entry i applies to
/// output `names[i]` of tab `tab_ids[i]`; empty strings mean unset. Called before `eval`,
/// like `geoscript_repl_set_gizmo_values`.
#[wasm_bindgen]
pub fn geoscript_repl_set_texture_params(
  ctx: *mut GeoscriptReplCtx,
  tab_ids: Vec<String>,
  names: Vec<String>,
  min_filters: Vec<String>,
  mag_filters: Vec<String>,
  formats: Vec<String>,
) {
  let ctx = unsafe { &mut *ctx };
  let mut map: FxHashMap<String, InjectedTextureParams> = FxHashMap::default();
  for i in 0..tab_ids.len() {
    let parse_filter = |s: &str| {
      (!s.is_empty())
        .then(|| TextureFilter::from_name(s).ok())
        .flatten()
    };
    let params = InjectedTextureParams {
      min_filter: parse_filter(&min_filters[i]),
      // GL mag filters can't have mipmap modes; drop anything else rather than erroring.
      mag_filter: parse_filter(&mag_filters[i])
        .filter(|f| matches!(f, TextureFilter::Nearest | TextureFilter::Linear)),
      format: (!formats[i].is_empty())
        .then(|| TextureFormat::from_name(&formats[i]).ok())
        .flatten(),
    };
    map.insert(format!("{}\0{}", tab_ids[i], names[i]), params);
  }
  *ctx.geo_ctx.injected_texture_params.borrow_mut() = map;
}

/// `[min_filter, mag_filter, format]` for a rendered texture; empty strings mean unset
/// (consumer defaults apply).
#[wasm_bindgen]
pub fn geoscript_get_rendered_texture_gpu_params(
  ctx: *const GeoscriptReplCtx,
  tex_ix: usize,
) -> Vec<String> {
  let ctx = unsafe { &*ctx };
  let textures = ctx.geo_ctx.rendered_textures.inner.borrow();
  let tex = &textures[tex_ix].texture;
  vec![
    tex
      .min_filter
      .map(|f| f.as_str().to_owned())
      .unwrap_or_default(),
    tex
      .mag_filter
      .map(|f| f.as_str().to_owned())
      .unwrap_or_default(),
    tex
      .format
      .map(|f| f.as_str().to_owned())
      .unwrap_or_default(),
  ]
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
  /// Per-channel stats table (see `TexStats::to_wire`) for `image_levels`; null otherwise.
  stats: Option<Vec<f32>>,
  has_override: bool,
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
    geoscript::ControlKind::Ramp => "ramp",
    geoscript::ControlKind::ImageLevels => "image_levels",
    geoscript::ControlKind::UvParams => "uv_params",
  }
  .to_owned();
  // Ramp + uv_params values can't ride the float lane; their editor form is `str_value` JSON.
  if matches!(
    c.kind,
    geoscript::ControlKind::Ramp | geoscript::ControlKind::UvParams
  ) {
    return RenderedControlWire {
      source_module: c.source_module.clone(),
      handle_id: c.handle_id.clone(),
      kind,
      label: c.label.clone(),
      value: Vec::new(),
      str_value: match c.kind {
        geoscript::ControlKind::Ramp => geoscript::ramp_control_value_json(&c.current_value),
        _ => geoscript::uv_params_control_value_json(&c.current_value),
      },
      min: None,
      max: None,
      step: None,
      style: None,
      options: Vec::new(),
      stats: None,
      has_override: c.has_override,
    }
    .serialize_json();
  }
  let (value, str_value): (Vec<f32>, Option<String>) = match &c.current_value {
    Value::Float(f) => (vec![*f], None),
    Value::Int(i) => (vec![*i as f32], None),
    Value::Bool(b) => (vec![if *b { 1. } else { 0. }], None),
    Value::Vec3(v) => (vec![v.x, v.y, v.z], None),
    Value::String(s) => (Vec::new(), Some(s.clone())),
    Value::Map(_) => (
      geoscript::image_levels_control_value(&c.current_value).unwrap_or_default(),
      None,
    ),
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
    stats: c.stats.as_ref().map(|s| s.to_wire()),
    has_override: c.has_override,
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
pub fn geoscript_repl_get_prelude(kind: String) -> String {
  prelude_for_kind(&kind).to_owned()
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

  #[test]
  fn exports_json_is_own_bindings_in_declaration_order() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;

    geoscript_repl_set_ambient_scope_from_sources(p, vec!["base = 10".to_owned()], None).unwrap();
    geoscript_repl_parse_program(
      p,
      "z = base + 1\na = z * 2\nbase = 99\na + z".to_owned(),
      None,
    );
    geoscript_repl_eval(p, None);
    ctx.last_result.as_ref().unwrap();

    // Exactly the program's own bindings, in declaration order. `base` is included even
    // though the ambient scope also defines it (the program rebinds it); ambient-only
    // names like `pi` are not.
    let json = geoscript_repl_get_exports_json(p, 4);
    let (zi, ai, bi) = (
      json.find("\"z\"").unwrap(),
      json.find("\"a\"").unwrap(),
      json.find("\"base\"").unwrap(),
    );
    assert!(zi < ai && ai < bi, "declaration order violated: {json}");
    assert!(!json.contains("\"pi\""), "ambient binding leaked: {json}");
    assert!(json[bi..].contains("99"), "rebound value missing: {json}");

    // Last-statement value: a + z = 22 + 11.
    assert!(geoscript_repl_get_last_value_json(p, 4).contains("33"));
  }

  fn set_modules(p: *mut GeoscriptReplCtx, modules: &[(&str, &str)]) {
    geoscript_repl_set_module_sources(
      p,
      modules.iter().map(|(n, _)| n.to_string()).collect(),
      modules.iter().map(|(_, s)| s.to_string()).collect(),
    );
  }

  fn set_tab_ambients(p: *mut GeoscriptReplCtx, tabs: &[(&str, &str)]) {
    geoscript_repl_set_tab_ambient_scopes(
      p,
      tabs.iter().map(|(id, _)| id.to_string()).collect(),
      tabs.iter().map(|_| String::new()).collect(),
      tabs.iter().map(|(_, g)| g.to_string()).collect(),
    )
    .unwrap();
  }

  fn eval_last_value(p: *mut GeoscriptReplCtx, src: &str, root_module: &str) -> String {
    geoscript_repl_parse_program(p, src.to_owned(), None);
    geoscript_repl_eval(p, Some(root_module.to_owned()));
    unsafe { (*p).last_result.as_ref().unwrap() };
    geoscript_repl_get_last_value_json(p, 4)
  }

  #[test]
  fn per_tab_ambient_scopes_isolate_tabs() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;

    set_modules(
      p,
      &[("t1:m", "export v1 = base"), ("t2:m", "export v2 = base")],
    );
    set_tab_ambients(p, &[("t2", "base = 2"), ("t1", "base = 1")]);

    // The bare import resolves within the entry's own tab; the qualified one crosses tabs,
    // and each module must see its own tab's `_globals`.
    let json = eval_last_value(
      p,
      "import { v1 } from \"m\"\nimport { v2 } from \"t2:m\"\nv1 * 100 + v2",
      "t1:_root",
    );
    assert!(json.contains("102"), "{json}");
  }

  #[test]
  fn tab_root_rng_stream_is_run_set_independent() {
    // Fresh ctx per run so this pins the eval path, not cache replay. t2's exported draw
    // must not depend on whether another rng-drawing tab evaluated before it.
    let solo = {
      let mut ctx = GeoscriptReplCtx::default();
      let p: *mut GeoscriptReplCtx = &mut ctx;
      set_modules(p, &[("t2:_root", "export x = randf()")]);
      set_tab_ambients(p, &[("t2", ""), ("t1", "")]);
      eval_last_value(p, "import { x } from \"t2:_root\"\nx", "t1:_root")
    };

    let with_earlier_dep = {
      let mut ctx = GeoscriptReplCtx::default();
      let p: *mut GeoscriptReplCtx = &mut ctx;
      set_modules(
        p,
        &[
          ("t2:_root", "export x = randf()"),
          ("t3:_root", "y = randf()"),
        ],
      );
      set_tab_ambients(p, &[("t3", ""), ("t2", ""), ("t1", "")]);
      eval_last_value(
        p,
        "import { } from \"t3:_root\"\nimport { x } from \"t2:_root\"\nx",
        "t1:_root",
      )
    };

    assert_eq!(solo, with_earlier_dep);
    assert!(solo.contains("float"), "{solo}");
  }

  #[test]
  fn tab_ambient_change_reevaluates_cached_importers() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;

    let modules: &[(&str, &str)] = &[
      ("ta:m", "export v = base"),
      ("tb:n", "import { v } from \"ta:m\"\nexport w = v + 1"),
    ];
    set_modules(p, modules);
    set_tab_ambients(p, &[("ta", "base = 5"), ("tb", "")]);
    let json = eval_last_value(p, "import { w } from \"n\"\nw", "tb:_root");
    assert!(json.contains("6"), "{json}");

    // Only ta's ambient changes; tb:n's source hash and tb's ambient are untouched, so a
    // stale cache would replay w = 6. The prefix eviction must cascade to importers.
    geoscript_repl_reset(p);
    set_modules(p, modules);
    set_tab_ambients(p, &[("ta", "base = 50"), ("tb", "")]);
    let json = eval_last_value(p, "import { w } from \"n\"\nw", "tb:_root");
    assert!(json.contains("51"), "{json}");
  }

  #[test]
  fn legacy_vs_tab_ambient_stream_parity() {
    // A single-tab run through the new per-tab path must draw the same stream the legacy
    // single-scope path did — the corpus gate depends on this byte-for-byte.
    let legacy = {
      let mut ctx = GeoscriptReplCtx::default();
      let p: *mut GeoscriptReplCtx = &mut ctx;
      geoscript_repl_reset(p);
      set_modules(p, &[("scene:child", "export c = randf()")]);
      geoscript_repl_set_ambient_scope_from_sources(
        p,
        vec![prelude_for_kind("mesh").to_owned(), "g = 1".to_owned()],
        Some("scene:_root".to_owned()),
      )
      .unwrap();
      eval_last_value(p, "import { c } from \"child\"\nc + randf()", "scene:_root")
    };
    let per_tab = {
      let mut ctx = GeoscriptReplCtx::default();
      let p: *mut GeoscriptReplCtx = &mut ctx;
      geoscript_repl_reset(p);
      set_modules(p, &[("scene:child", "export c = randf()")]);
      geoscript_repl_set_tab_ambient_scopes(
        p,
        vec!["scene".to_owned()],
        vec!["mesh".to_owned()],
        vec!["g = 1".to_owned()],
      )
      .unwrap();
      eval_last_value(p, "import { c } from \"child\"\nc + randf()", "scene:_root")
    };
    assert_eq!(legacy, per_tab);
  }

  fn ramp_wire(spec: &str) -> String {
    #[derive(SerJson)]
    struct W {
      kind: String,
      str_value: String,
    }
    W {
      kind: "ramp".to_owned(),
      str_value: spec.to_owned(),
    }
    .serialize_json()
  }

  fn const_ramp_spec(v: f32) -> String {
    format!(
      r#"{{"scalar":false,"stops":[{{"pos":0.0,"value":[{v},{v},{v}],"ease":"linear"}},{{"pos":1.0,"value":[{v},{v},{v}],"ease":"linear"}}],"extend":"clamp","space":"linear"}}"#
    )
  }

  /// Pixel-buffer Rc addresses of every rendered texture slice after evaling `src` fresh
  /// on a warm ctx (const-eval cache persists across the reset, like real reruns).
  fn eval_rendered_pixel_ptrs(p: *mut GeoscriptReplCtx, src: &str) -> Vec<u64> {
    geoscript_repl_reset(p);
    geoscript_repl_parse_program(p, src.to_owned(), None);
    geoscript_repl_eval(p, None);
    let ctx = unsafe { &*p };
    ctx.last_result.as_ref().unwrap();
    let out = ctx
      .geo_ctx
      .rendered_textures
      .inner
      .borrow()
      .iter()
      .flat_map(|rt| {
        std::iter::once(&rt.texture)
          .chain(rt.extra_slices.iter())
          .map(|t| t.storage_id())
      })
      .collect::<Vec<_>>();
    assert!(!out.is_empty());
    out
  }

  const LAZY_STACK_HEADER: &str =
    "n = 32\nf0 = texture(n, n, |uv| fbm(pos=uv))\nramp = color_ramp(stops=[[-1., \
     srgb(0x504d4c)], [1., srgb(0xdfd9d3)]])\nlayers = 0..4 -> |i| ramp(f0 * ((i + 1) / 4.))\n";

  /// A pure lazy map piped into a side-effectful consumer gets pre-collected into the
  /// const-eval cache, so warm runs replay the computed slices instead of re-running the
  /// per-element work inside the render impl.
  #[test]
  fn lazy_seq_into_render_stack_replays_from_cache() {
    let src = format!(
      "{LAZY_STACK_HEADER}layers | render_texture_stack(name=\"s\")\nlayers | first | \
       render_texture(name='x')"
    );
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let first = eval_rendered_pixel_ptrs(p, &src);
    let second = eval_rendered_pixel_ptrs(p, &src);
    assert_eq!(first.len(), 5);
    assert_eq!(first, second, "warm run must replay all cached slices");
  }

  #[test]
  fn direct_call_seq_arg_replays_from_cache() {
    let src = format!("{LAZY_STACK_HEADER}render_texture_stack(layers, name=\"s\")");
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let first = eval_rendered_pixel_ptrs(p, &src);
    let second = eval_rendered_pixel_ptrs(p, &src);
    assert_eq!(first.len(), 4);
    assert_eq!(first, second);
  }

  /// Fold keys for rng-free seq computations must not embed rng-stream position: inserting
  /// an unrelated draw upstream shifts the fold-world rng state but must not invalidate the
  /// cached map/collect chain (pre-sharpening, the blanket `Value::Sequence` poison did).
  #[test]
  fn pure_seq_fold_cache_survives_unrelated_rng_shift() {
    let head = "n = 16\nf0 = texture(n, n, |uv| fbm(pos=uv))\n";
    let tail =
      "layers = 0..4 -> |i| f0 * ((i + 1) / 4.)\nlayers | render_texture_stack(name=\"s\")";
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let first = eval_rendered_pixel_ptrs(p, &format!("{head}{tail}"));
    let second = eval_rendered_pixel_ptrs(p, &format!("{head}shift = randf()\n{tail}"));
    assert_eq!(first.len(), 4);
    assert_eq!(
      first, second,
      "rng-position-independent seq folds must replay across unrelated rng edits"
    );
  }

  /// Wrapped callbacks (partial application over a pure builtin) must get the same precise
  /// rng analysis as bare closures — no fallback to the blanket poison.
  #[test]
  fn paf_callback_seq_replays_from_cache() {
    let src = "n = 16\nf0 = texture(n, n, |uv| fbm(pos=uv))\nscale = |a, b| f0 * (a * b)\nlayers \
               = [0.25, 0.5, 0.75, 1.] -> scale(0.5)\nlayers | render_texture_stack(name=\"s\")";
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let first = eval_rendered_pixel_ptrs(p, src);
    let second = eval_rendered_pixel_ptrs(p, src);
    assert_eq!(first.len(), 4);
    assert_eq!(first, second);
  }

  /// An rng-drawing callback must NOT be pre-collected: draws stay at consumption time and
  /// each run recomputes (and redraws), exactly as before.
  #[test]
  fn rng_lazy_seq_into_render_stack_stays_lazy() {
    let src = "n = 8\nf0 = texture(n, n, |uv| fbm(pos=uv))\nlayers = 0..3 -> |i| f0 * \
               randf()\nlayers | render_texture_stack(name=\"s\")";
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let first = eval_rendered_pixel_ptrs(p, src);
    let second = eval_rendered_pixel_ptrs(p, src);
    assert_eq!(first.len(), 3);
    assert_ne!(
      first, second,
      "rng seq must stay lazy and recompute per run"
    );
  }

  #[test]
  fn input_ramp_warm_runs_hit_const_cache() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let src = "n = 32\nshade = input_color_ramp(\"s\", default=[[-1., vec3(0.1, 0.1, 0.1)], [1., \
               vec3(0.9, 0.9, 0.9)]])\nt = texture(n, n, |uv| fbm(pos=uv) | shade)\nt | \
               render_texture(name=\"d\")";

    let run = |p: *mut GeoscriptReplCtx| -> usize {
      geoscript_repl_reset(p);
      geoscript_repl_parse_program(p, src.to_owned(), None);
      geoscript_repl_eval(p, None);
      let ctx = unsafe { &*p };
      ctx.last_result.as_ref().unwrap();
      // The control must register every run even when the whole chain cache-hits.
      assert_eq!(ctx.geo_ctx.rendered_controls.len(), 1);
      Rc::as_ptr(&ctx.geo_ctx.rendered_textures.inner.borrow()[0].texture) as usize
    };

    let first = run(p);
    let second = run(p);
    assert_eq!(first, second, "warm run must replay the cached texture");
  }

  #[test]
  fn input_ramp_injected_change_invalidates_and_rewarms() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let src = "n = 32\nshade = input_color_ramp(\"s\", default=[[-1., vec3(0.1, 0.1, 0.1)], [1., \
               vec3(0.9, 0.9, 0.9)]])\nt = texture(n, n, |uv| fbm(pos=uv) | shade)\nt | \
               render_texture(name=\"d\")";

    let run = |p: *mut GeoscriptReplCtx, inject: Option<&str>| -> (usize, f32) {
      geoscript_repl_reset(p);
      if let Some(spec) = inject {
        geoscript_repl_set_gizmo_values(
          p,
          vec!["_root".to_owned()],
          vec!["s".to_owned()],
          vec![ramp_wire(spec)],
        );
      }
      geoscript_repl_parse_program(p, src.to_owned(), None);
      geoscript_repl_eval(p, None);
      let ctx = unsafe { &*p };
      ctx.last_result.as_ref().unwrap();
      let textures = ctx.geo_ctx.rendered_textures.inner.borrow();
      (
        Rc::as_ptr(&textures[0].texture) as usize,
        textures[0].texture.as_interleaved()[0],
      )
    };

    let (default_ptr, _) = run(p, None);
    let (a_ptr, a_px) = run(p, Some(&const_ramp_spec(0.25)));
    assert_ne!(default_ptr, a_ptr, "injected value must invalidate");
    assert!((a_px - 0.25).abs() < 1e-4, "{a_px}");
    let (a2_ptr, _) = run(p, Some(&const_ramp_spec(0.25)));
    assert_eq!(a_ptr, a2_ptr, "same injected value must re-warm");
    let (b_ptr, b_px) = run(p, Some(&const_ramp_spec(0.75)));
    assert_ne!(a_ptr, b_ptr, "changed injected value must recompute");
    assert!((b_px - 0.75).abs() < 1e-4, "{b_px}");
  }

  #[test]
  fn input_image_levels_injection_roundtrip() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let src = "g = texture(8, 8, |uv| uv.x)\nout = input_image_levels(\"lv\", g)\nout | \
               render_texture(name=\"d\")";
    let levels_wire = |vals: [f32; 5]| -> String {
      #[derive(SerJson)]
      struct W {
        kind: String,
        value: Vec<f32>,
      }
      W {
        kind: "image_levels".to_owned(),
        value: vals.to_vec(),
      }
      .serialize_json()
    };

    let run = |p: *mut GeoscriptReplCtx, inject: Option<[f32; 5]>| -> (usize, f32, String) {
      geoscript_repl_reset(p);
      if let Some(vals) = inject {
        geoscript_repl_set_gizmo_values(
          p,
          vec!["_root".to_owned()],
          vec!["lv".to_owned()],
          vec![levels_wire(vals)],
        );
      }
      geoscript_repl_parse_program(p, src.to_owned(), None);
      geoscript_repl_eval(p, None);
      let ctx = unsafe { &*p };
      ctx.last_result.as_ref().unwrap();
      let control_json = geoscript_repl_get_rendered_control(p, 0);
      let textures = ctx.geo_ctx.rendered_textures.inner.borrow();
      (
        Rc::as_ptr(&textures[0].texture) as usize,
        textures[0].texture.as_interleaved()[0],
        control_json,
      )
    };

    // `input_image_levels` deliberately stays runtime (not in FOLDABLE_INPUT_NAMES): the
    // levels pass itself is the cheap per-run cost, so assert value semantics rather than
    // pointer-stable replay.
    let (_, default_px, control_json) = run(p, None);
    assert!((default_px - 0.5 / 8.).abs() < 1e-6);
    assert!(
      control_json.contains("\"kind\":\"image_levels\""),
      "{control_json}"
    );
    assert!(control_json.contains("\"stats\":["), "{control_json}");

    let (_, a_px, a_json) = run(p, Some([0., 1., 0., 0.5, 1.]));
    assert!((a_px - 0.5 * 0.5 / 8.).abs() < 1e-6, "{a_px}");
    assert!(
      a_json.contains("\"value\":[0.0,1.0,0.0,0.5,1.0]"),
      "{a_json}"
    );
    let (_, a2_px, _) = run(p, Some([0., 1., 0., 0.5, 1.]));
    assert_eq!(a_px, a2_px);
    let (_, b_px, _) = run(p, Some([0., 1., 0., 2., 1.]));
    assert!((b_px - 2. * 0.5 / 8.).abs() < 1e-6, "{b_px}");
  }

  #[test]
  fn texture_params_apply_and_survive_cache_replay() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let module_src = "texture(2, 2, |uv| uv.x) | render_texture(name=\"d\")";
    let entry = "import { } from \"child\"";

    let run = |p: *mut GeoscriptReplCtx, params: Option<(&str, &str)>| -> (Vec<String>, u64) {
      geoscript_repl_reset(p);
      geoscript_repl_set_module_sources(p, vec!["t:child".to_owned()], vec![module_src.to_owned()]);
      if let Some((min, format)) = params {
        geoscript_repl_set_texture_params(
          p,
          vec!["t".to_owned()],
          vec!["d".to_owned()],
          vec![min.to_owned()],
          vec!["nearest".to_owned()],
          vec![format.to_owned()],
        );
      }
      geoscript_repl_parse_program(p, entry.to_owned(), None);
      geoscript_repl_eval(p, Some("t:_root".to_owned()));
      let ctx = unsafe { &*p };
      ctx.last_result.as_ref().unwrap();
      let pixels_ptr = ctx.geo_ctx.rendered_textures.inner.borrow()[0]
        .texture
        .storage_id();
      (geoscript_get_rendered_texture_gpu_params(p, 0), pixels_ptr)
    };

    let (params1, px1) = run(p, Some(("nearest", "r8")));
    assert_eq!(params1, ["nearest", "nearest", "r8"]);
    // Warm cache hit with different params: the replayed push must get re-baked.
    let (params2, px2) = run(p, Some(("linear_mipmap_linear", "r32f")));
    assert_eq!(params2, ["linear_mipmap_linear", "nearest", "r32f"]);
    assert_eq!(px1, px2, "pixels must replay from cache, not re-synthesize");
    // No injection: fields come back unset.
    let (params3, _) = run(p, None);
    assert_eq!(params3, ["", "", ""]);
  }

  #[test]
  fn module_controls_replay_on_warm_cache_hit() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let module_src =
      "r = input_color_ramp(\"s\", default=[[0., vec3(0.)], [1., vec3(1.)]])\nexport v = r(0.5)";
    let entry = "import { v } from \"child\"\nv";

    // Run 2 replays `t:child` from the module cache; its fold-time-registered control
    // must be in the replay set, not just the fresh-eval side-effect diff.
    for run in 0..2 {
      geoscript_repl_reset(p);
      geoscript_repl_set_module_sources(p, vec!["t:child".to_owned()], vec![module_src.to_owned()]);
      geoscript_repl_parse_program(p, entry.to_owned(), None);
      geoscript_repl_eval(p, Some("t:_root".to_owned()));
      unsafe { (*p).last_result.as_ref().unwrap() };
      assert_eq!(
        geoscript_repl_get_rendered_control_count(p),
        1,
        "control missing on run {run}"
      );
    }
  }

  #[test]
  fn module_cache_not_stale_on_ramp_control_edit() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let module_src =
      "r = input_color_ramp(\"s\", default=[[0., vec3(0.)], [1., vec3(1.)]])\nexport v = r(0.5)";
    let entry = "import { v } from \"child\"\nv";

    let run = |p: *mut GeoscriptReplCtx, spec: &str| -> String {
      geoscript_repl_reset(p);
      geoscript_repl_set_module_sources(p, vec!["t:child".to_owned()], vec![module_src.to_owned()]);
      geoscript_repl_set_gizmo_values(
        p,
        vec!["t:child".to_owned()],
        vec!["s".to_owned()],
        vec![ramp_wire(spec)],
      );
      geoscript_repl_parse_program(p, entry.to_owned(), None);
      geoscript_repl_eval(p, Some("t:_root".to_owned()));
      unsafe { (*p).last_result.as_ref().unwrap() };
      geoscript_repl_get_last_value_json(p, 4)
    };

    let a = run(p, &const_ramp_spec(0.25));
    assert!(a.contains("0.25"), "{a}");
    // Same spec: the module may replay from cache, but the value must stay right.
    let a2 = run(p, &const_ramp_spec(0.25));
    assert!(a2.contains("0.25"), "{a2}");
    // Edited spec: a stale sentinel-hashed gizmo read would replay 0.25 here.
    let b = run(p, &const_ramp_spec(0.75));
    assert!(b.contains("0.75"), "{b}");
  }

  #[test]
  fn input_color_ramp_control_wire_roundtrip() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let src =
      "shade = input_color_ramp(\"shade\", default=[[0., vec3(0.)], [1., vec3(1.)]])\nshade(0.5)";
    geoscript_repl_parse_program(p, src.to_owned(), None);
    geoscript_repl_eval(p, None);
    unsafe { (*p).last_result.as_ref().unwrap() };

    // Default flows out as an editable spec.
    assert_eq!(geoscript_repl_get_rendered_control_count(p), 1);
    let wire = geoscript_repl_get_rendered_control(p, 0);
    assert!(wire.contains("ramp") && wire.contains("oklab"), "{wire}");

    // Inject an edited spec (constant color, linear space for exactness) and re-eval:
    // the injected value must override the default.
    #[derive(SerJson)]
    struct W {
      kind: String,
      str_value: String,
    }
    let spec = r#"{"scalar":false,"stops":[{"pos":0.0,"value":[0.25,0.5,0.75],"ease":"linear"},{"pos":1.0,"value":[0.25,0.5,0.75],"ease":"linear"}],"extend":"clamp","space":"linear"}"#;
    geoscript_repl_set_gizmo_values(
      p,
      vec!["_root".to_owned()],
      vec!["shade".to_owned()],
      vec![W {
        kind: "ramp".to_owned(),
        str_value: spec.to_owned(),
      }
      .serialize_json()],
    );
    geoscript_repl_parse_program(p, src.to_owned(), None);
    geoscript_repl_eval(p, None);
    unsafe { (*p).last_result.as_ref().unwrap() };
    let json = geoscript_repl_get_last_value_json(p, 4);
    assert!(
      json.contains("0.25") && json.contains("0.5") && json.contains("0.75"),
      "{json}"
    );
  }

  /// Both directions of the fixed-order `[in_lo, in_hi, out_lo, out_hi, gamma]` wire array
  /// go through the geoscript crate's helpers; this pins the order across that boundary.
  #[test]
  fn input_image_levels_control_wire_roundtrip() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;
    let src = "g = texture(4, 1, |uv| uv.x)\nlv = input_image_levels(\"lv\", g)\nlv[0]";
    geoscript_repl_parse_program(p, src.to_owned(), None);
    geoscript_repl_eval(p, None);
    unsafe { (*p).last_result.as_ref().unwrap() };

    // Identity defaults flow out in wire order.
    let wire = geoscript_repl_get_rendered_control(p, 0);
    assert!(wire.contains("image_levels"), "{wire}");
    assert!(
      wire.contains("\"value\":[0.0,1.0,0.0,1.0,1.0]"),
      "identity in wire order: {wire}"
    );
    assert!(wire.contains("\"has_override\":false"), "{wire}");

    // in_hi=0.5 doubles the black end, proving slot 1 drives in_hi and not some other param.
    #[derive(SerJson)]
    struct W {
      kind: String,
      value: Vec<f32>,
    }
    geoscript_repl_set_gizmo_values(
      p,
      vec!["_root".to_owned()],
      vec!["lv".to_owned()],
      vec![W {
        kind: "image_levels".to_owned(),
        value: vec![0., 0.5, 0., 1., 1.],
      }
      .serialize_json()],
    );
    geoscript_repl_parse_program(p, src.to_owned(), None);
    geoscript_repl_eval(p, None);
    unsafe { (*p).last_result.as_ref().unwrap() };
    let json = geoscript_repl_get_last_value_json(p, 4);
    assert!(
      json.contains("0.25"),
      "in_hi=0.5 must double the black end: {json}"
    );
    // No reset between runs here, so the second eval's control is the last entry.
    let wire =
      geoscript_repl_get_rendered_control(p, geoscript_repl_get_rendered_control_count(p) - 1);
    assert!(wire.contains("\"has_override\":true"), "{wire}");
  }

  #[test]
  fn entry_const_fold_seeds_from_entry_tab_ambient() {
    let mut ctx = GeoscriptReplCtx::default();
    let p: *mut GeoscriptReplCtx = &mut ctx;

    // Conflicting single-scope fallback: if `current_module` weren't set before
    // `optimize_ast`, the const-folder would seed `k = 1` from it and fold the wrong value.
    geoscript_repl_set_ambient_scope_from_sources(p, vec!["k = 1".to_owned()], None).unwrap();
    set_modules(p, &[]);
    set_tab_ambients(p, &[("t", "k = 3")]);
    let json = eval_last_value(p, "k * 2", "t:_root");
    assert!(json.contains("6"), "{json}");
  }
}
