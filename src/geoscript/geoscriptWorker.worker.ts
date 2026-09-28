import * as Comlink from 'comlink';

import { isWasmTrap } from './wasmTrap';

import { compute_convex_hull_mesh, initManifoldWasm, setManifoldWasmURL } from './manifold';
import type {
  RenderedMeshPayload,
  RenderedTexturePayload,
  TextureDetail,
  WorkerRunJob,
  WorkerRunPayload,
} from './runner/types';
import * as Geoscript from 'src/viz/wasmComp/geoscript_repl';

import { initGeodesics, setGeodesicsWasmURL } from './geodesics';
import { initCGAL, setCGALWasmURL } from 'src/viz/wasm/cgal/cgal';
import { initClipper2, setClipper2WasmURL } from 'src/viz/wasm/clipper2/clipper2';
import { initUVUnwrap, setUVUnwrapWasmURL } from './uvUnwrap';
import { initUVSolvers, setUVSolversWasmURL } from './uvSolvers';
import { initImageData } from './imageData';
import { initModelData, setModelDataURLs } from './modelData';
import { textToSvg } from './text_to_path';
import { initMeshopt } from './meshopt';
import type { GeoscriptWorkerWasmURLs } from 'src/viz/wasmComp/wasmAssetURLs';

// Wasm asset URLs are passed in by the main thread via `init()` (not imported
// with `?url` here) so Vite emits each wasm only into the main bundle's asset
// graph.  This keeps the URL the worker fetches identical to the one preloaded
// by `<link rel=preload>` in the scene route's HTML.
let geoscriptReplWasmURL: string | null = null;

const initGeoscript = async () => {
  if (!geoscriptReplWasmURL) {
    throw new Error('geoscript_repl wasm URL not set; pass urls to worker init()');
  }
  // Pass `fetch(url)` directly so wasm-bindgen uses `WebAssembly.instantiateStreaming`.
  // With the `<link rel="preload">` from the scene route, the fetch is a cache hit.
  await Geoscript.default({ module_or_path: fetch(geoscriptReplWasmURL) });
  return Geoscript;
};

const filterNils = <T>(arr: (T | null | undefined)[]): T[] => arr.filter((x): x is T => x != null);

// `console_error_panic_hook` logs the real `panicked at …` message (with location + JS
// stack) to `console.error` synchronously, just before the wasm trap surfaces in JS as a
// bare `RuntimeError: unreachable`. Capture that string here so the actual panic — not the
// useless trap — reaches the caller. The original `console.error` still runs, so worker
// console logging (and anything scraping it, e.g. the headless harness) is unaffected.
let lastWasmPanic: string | null = null;
{
  const orig = console.error.bind(console);
  console.error = (...args: unknown[]) => {
    const first = args[0];
    if (typeof first === 'string' && first.includes('panicked at')) {
      lastWasmPanic = args.map(a => (typeof a === 'string' ? a : String(a))).join(' ');
    }
    orig(...args);
  };
}

/** Prefer the captured Rust panic message over the opaque `unreachable` trap it throws as. */
const enrichWasmError = (err: unknown): Error => {
  const panic = lastWasmPanic;
  lastWasmPanic = null;
  if (panic) {
    const e = new Error(panic);
    e.name = 'WasmPanic';
    return e;
  }
  return err instanceof Error ? err : new Error(String(err));
};

export interface GeoscriptAsyncDeps {
  meshopt?: boolean;
  geodesics?: boolean;
  cgal?: boolean;
  text_to_path?: boolean;
  clipper2?: boolean;
  uv_unwrap?: boolean;
  uv_solvers?: boolean;
  model_data?: boolean;
  image_data?: boolean;
}

const initAsyncDeps = (
  deps: GeoscriptAsyncDeps,
  argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>>
) => {
  const promises: Promise<void>[] = [];
  if (deps.meshopt) promises.push(initMeshopt());
  if (deps.geodesics) {
    promises.push(initGeodesics());
  }
  if (deps.cgal) {
    const cgalInit = initCGAL();
    if (cgalInit instanceof Promise) {
      promises.push(cgalInit);
    }
  }
  if (deps.clipper2) {
    const clipperInit = initClipper2();
    if (clipperInit instanceof Promise) {
      promises.push(clipperInit);
    }
  }
  if (deps.uv_unwrap) {
    promises.push(initUVUnwrap());
  }
  if (deps.uv_solvers) {
    promises.push(initUVSolvers());
  }
  if (deps.model_data) {
    promises.push(initModelData(argsByKey.model_data));
  }
  if (deps.image_data) {
    promises.push(initImageData(argsByKey.image_data));
  }
  if (deps.text_to_path) {
    const args = argsByKey.text_to_path;
    if (!args) {
      throw new Error('text_to_path dependency requires arguments');
    }

    const [text, fontFamily, fontSize, fontWeight, fontStyle, letterSpacing] = args;

    const convertedFontWeight = fontWeight
      ? isNaN(Number(fontWeight))
        ? fontWeight
        : Number(fontWeight)
      : undefined;

    promises.push(
      textToSvg(text, {
        fontFamily,
        fontSize: fontSize ? +fontSize : undefined,
        fontWeight: convertedFontWeight,
        fontStyle: (fontStyle || undefined) as 'normal' | 'italic' | 'oblique' | undefined,
        letterSpacing: letterSpacing ? +letterSpacing : undefined,
      })
    );
  }

  if (!promises.length) {
    return null;
  }

  return Promise.all(promises);
};

const depsOf = (names: (keyof GeoscriptAsyncDeps)[]): GeoscriptAsyncDeps =>
  Object.fromEntries(names.map(n => [n, true]));

/** `__GEOTOY_UNINITIALIZED_MODULE__:<dep>[||__||arg…]` → the dep to init before re-running. */
const parseUninitializedDep = (err: string) => {
  const name = /__GEOTOY_UNINITIALIZED_MODULE__:(\w+)/.exec(err)?.[1] as keyof GeoscriptAsyncDeps | undefined;
  if (!name) {
    return null;
  }
  return { name, args: err.includes('||__||') ? err.split('||__||').slice(1) : undefined };
};

const emptyPayload = (error: string, wasmTrap: boolean): WorkerRunPayload => ({
  error,
  wasmTrap,
  asyncDepRetries: 0,
  durationMs: 0,
  usedDepsBitmask: 0,
  phases: { setup: 0, ambient: 0, evalWall: 0, extract: 0 },
  constEvalCache: { entries: 0, bytes: 0, maxBytes: 0 },
  vectorizeReports: [],
  meshes: [],
  paths: [],
  lights: [],
  textures: [],
  gizmos: [],
  controls: [],
});

const extractMesh = (ctxPtr: number, i: number, buffers: Transferable[]): RenderedMeshPayload => {
  const verts = Geoscript.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, i);
  const indices = Geoscript.geoscript_repl_get_rendered_mesh_indices(ctxPtr, i);
  const normals = Geoscript.geoscript_repl_get_rendered_mesh_normals(ctxPtr, i);
  const attrs: RenderedMeshPayload['attrs'] = [];
  const attrCount = Geoscript.geoscript_repl_get_rendered_mesh_attr_count(ctxPtr, i);
  for (let a = 0; a < attrCount; a += 1) {
    attrs.push({
      name: Geoscript.geoscript_repl_get_rendered_mesh_attr_name(ctxPtr, i, a),
      itemSize: Geoscript.geoscript_repl_get_rendered_mesh_attr_arity(ctxPtr, i, a),
      data: Geoscript.geoscript_repl_get_rendered_mesh_attr_data(ctxPtr, i, a),
    });
  }
  buffers.push(
    verts.buffer as ArrayBuffer,
    indices.buffer as ArrayBuffer,
    ...attrs.map(a => a.data.buffer as ArrayBuffer)
  );
  if (normals) {
    buffers.push(normals.buffer as ArrayBuffer);
  }
  return {
    verts,
    indices,
    normals,
    attrs,
    transform: Geoscript.geoscript_repl_get_rendered_mesh_transform(ctxPtr, i),
    material: Geoscript.geoscript_repl_get_rendered_mesh_material(ctxPtr, i),
    sourceModule: Geoscript.geoscript_repl_get_rendered_mesh_source_module(ctxPtr, i),
    meshId: Geoscript.geoscript_repl_get_rendered_mesh_id(ctxPtr, i),
  };
};

/** `detail: 'gpu'` skips the host-side-only products (value stats, and the f32 rgba
 *  expansion the 2D preview needs) — level loading uses none of them, and for a stack
 *  they dominate the export. */
const extractTexture = (
  ctxPtr: number,
  i: number,
  detail: TextureDetail,
  buffers: Transferable[]
): RenderedTexturePayload => {
  const [width, height, channels] = Geoscript.geoscript_get_rendered_texture_dims(ctxPtr, i);
  /** 1 for plain outputs; stacks concatenate their slices in `pixels`. */
  const layers = Geoscript.geoscript_get_rendered_texture_layers(ctxPtr, i);
  const stats =
    detail === 'full' ? Geoscript.geoscript_get_rendered_texture_stats(ctxPtr, i) : new Float32Array(0);
  const [minFilter, magFilter, format] = Geoscript.geoscript_get_rendered_texture_gpu_params(ctxPtr, i);
  /** SIMD-encoded in wasm for u8 materialization formats; empty for float formats. */
  const encodedRaw = Geoscript.geoscript_encode_rendered_texture_pixels(ctxPtr, i);
  const encoded = encodedRaw.length ? encodedRaw : undefined;
  // must mirror the wasm export's unset-format resolution
  const encodedFormat = encoded ? format || 'rgba8' : undefined;
  /** See `GeneratedTexture.rgba`. Kept for `rgba32f` even at `gpu` detail: there it *is*
   *  the materialization source, not a preview copy. */
  const rgba =
    channels === 3 && (detail === 'full' || format === 'rgba32f')
      ? Geoscript.geoscript_get_rendered_texture_pixels_rgba(ctxPtr, i)
      : undefined;
  /** A 3-channel output's raw pixels have no consumer: the 2D preview reads `rgba` and
   *  every materialization format reads `encoded` or `rgba` — except r32f/rg32f, which
   *  take channel 0/1 off the raw interleave. Skipping it is the single biggest chunk of
   *  the export for rgb stacks. */
  const rawFloatFormat = format === 'r32f' || format === 'rg32f';
  const pixels =
    channels === 3 && !rawFloatFormat
      ? undefined
      : Geoscript.geoscript_get_rendered_texture_pixels(ctxPtr, i);
  buffers.push(
    ...filterNils([pixels?.buffer, encoded?.buffer, rgba?.buffer, stats.buffer]).map(b => b as ArrayBuffer)
  );
  return {
    width,
    height,
    channels,
    layers,
    pixels,
    encoded,
    encodedFormat,
    rgba,
    name: Geoscript.geoscript_get_rendered_texture_name(ctxPtr, i),
    usage: Geoscript.geoscript_get_rendered_texture_usage(ctxPtr, i),
    wrap: Geoscript.geoscript_get_rendered_texture_wrap(ctxPtr, i),
    sourceModule: Geoscript.geoscript_get_rendered_texture_source_module(ctxPtr, i),
    textureId: Geoscript.geoscript_get_rendered_texture_id(ctxPtr, i),
    minFilter,
    magFilter,
    format,
    stats,
  };
};

/** One attempt, fully synchronous: nothing queued on the worker can interleave with it. */
const runAttempt = (
  ctxPtr: number,
  job: WorkerRunJob
): { payload: WorkerRunPayload; buffers: Transferable[] } => {
  const buffers: Transferable[] = [];
  const tStart = performance.now();
  if (job.clearConstEvalCache) {
    Geoscript.geoscript_repl_clear_const_eval_cache(ctxPtr);
  }
  if (job.materials) {
    Geoscript.geoscript_set_default_material(ctxPtr, job.materials.defaultName ?? undefined);
    Geoscript.geoscript_set_materials(ctxPtr, job.materials.available);
  }
  Geoscript.geoscript_repl_reset(ctxPtr);
  Geoscript.geoscript_repl_set_no_vectorize(ctxPtr, job.vectorize.disabled);
  Geoscript.geoscript_repl_set_verify(ctxPtr, job.vectorize.verify);
  Geoscript.geoscript_repl_set_vectorize_profile(ctxPtr, job.vectorize.profile);
  // Sent even when empty: `set_module_sources` is the only thing that clears the ctx's
  // registered sources, so skipping it would leave a previous run's modules resolvable.
  if (job.modules) {
    const names = Object.keys(job.modules);
    const sources = names.map(name => {
      const kind = job.modulePreludes?.[name];
      return kind
        ? `${Geoscript.geoscript_repl_get_prelude(kind)}\n${job.modules![name]}`
        : job.modules![name];
    });
    Geoscript.geoscript_repl_set_module_sources(ctxPtr, names, sources);
  }
  const tSetup = performance.now();

  lastWasmPanic = null;
  try {
    if (job.tabAmbients) {
      Geoscript.geoscript_repl_set_tab_ambient_scopes(
        ctxPtr,
        job.tabAmbients.map(t => t.tabId),
        job.tabAmbients.map(t => t.preludeKind),
        job.tabAmbients.map(t => t.globalsSource)
      );
    } else if (job.ambientSources?.length === 0) {
      Geoscript.geoscript_repl_clear_ambient_scope(ctxPtr);
    } else if (job.ambientSources) {
      Geoscript.geoscript_repl_set_ambient_scope_from_sources(ctxPtr, job.ambientSources, job.rootModuleName);
    }
  } catch (rawErr) {
    const err = enrichWasmError(rawErr);
    return { payload: emptyPayload(`Error building ambient scope: ${err}`, isWasmTrap(err)), buffers };
  }

  // Always sent so a previous run's handle values / texture params can't leak in.
  const modules: string[] = [];
  const handles: string[] = [];
  const valuesJson: string[] = [];
  for (const [mod, handleMap] of Object.entries(job.gizmoValues)) {
    for (const [handle, v] of Object.entries(handleMap)) {
      modules.push(mod);
      handles.push(handle);
      valuesJson.push(JSON.stringify(v));
    }
  }
  Geoscript.geoscript_repl_set_gizmo_values(ctxPtr, modules, handles, valuesJson);
  Geoscript.geoscript_repl_set_texture_params(
    ctxPtr,
    job.textureParams.map(e => e.tabId),
    job.textureParams.map(e => e.name),
    job.textureParams.map(e => e.minFilter ?? ''),
    job.textureParams.map(e => e.magFilter ?? ''),
    job.textureParams.map(e => e.format ?? '')
  );
  const tAmbient = performance.now();

  let durationMs = 0;
  let usedDepsBitmask = 0;
  lastWasmPanic = null;
  try {
    Geoscript.geoscript_repl_parse_program(ctxPtr, job.code, job.preludeKind);
    if (!Geoscript.geoscript_repl_has_err(ctxPtr)) {
      const start = performance.now();
      Geoscript.geoscript_repl_eval(ctxPtr, job.rootModuleName);
      durationMs = performance.now() - start;
      usedDepsBitmask = Geoscript.geoscript_repl_get_used_async_deps(ctxPtr);
    }
  } catch (rawErr) {
    const err = enrichWasmError(rawErr);
    const message = `Error evaluating code: ${err}`;
    console.error(message, err);
    return { payload: emptyPayload(message, isWasmTrap(err)), buffers };
  }
  const tEval = performance.now();
  const err = Geoscript.geoscript_repl_get_err(ctxPtr);
  if (err) {
    return { payload: emptyPayload(err, false), buffers };
  }

  const meshes: RenderedMeshPayload[] = [];
  const meshCount = Geoscript.geoscript_repl_get_rendered_mesh_count(ctxPtr);
  for (let i = 0; i < meshCount; i += 1) {
    meshes.push(extractMesh(ctxPtr, i, buffers));
  }
  const paths: WorkerRunPayload['paths'] = [];
  const pathCount = Geoscript.geoscript_get_rendered_path_count(ctxPtr);
  for (let i = 0; i < pathCount; i += 1) {
    const verts = Geoscript.geoscript_get_rendered_path(ctxPtr, i);
    buffers.push(verts.buffer as ArrayBuffer);
    paths.push({
      verts,
      pathId: Geoscript.geoscript_get_rendered_path_id(ctxPtr, i),
      sourceModule: Geoscript.geoscript_get_rendered_path_source_module(ctxPtr, i),
    });
  }
  const lights: WorkerRunPayload['lights'] = [];
  const lightCount = Geoscript.geoscript_get_rendered_light_count(ctxPtr);
  for (let i = 0; i < lightCount; i += 1) {
    lights.push({
      light: JSON.parse(Geoscript.geoscript_get_rendered_light(ctxPtr, i)),
      lightId: Geoscript.geoscript_get_rendered_light_id(ctxPtr, i),
      sourceModule: Geoscript.geoscript_get_rendered_light_source_module(ctxPtr, i),
    });
  }
  const textures: RenderedTexturePayload[] = [];
  const textureCount = Geoscript.geoscript_get_rendered_texture_count(ctxPtr);
  for (let i = 0; i < textureCount; i += 1) {
    textures.push(extractTexture(ctxPtr, i, job.textureDetail, buffers));
  }
  const gizmos: WorkerRunPayload['gizmos'] = [];
  const gizmoCount = Geoscript.geoscript_repl_get_rendered_gizmo_count(ctxPtr);
  for (let i = 0; i < gizmoCount; i += 1) {
    gizmos.push(JSON.parse(Geoscript.geoscript_repl_get_rendered_gizmo(ctxPtr, i)));
  }
  const controls: WorkerRunPayload['controls'] = [];
  const controlCount = Geoscript.geoscript_repl_get_rendered_control_count(ctxPtr);
  for (let i = 0; i < controlCount; i += 1) {
    controls.push(JSON.parse(Geoscript.geoscript_repl_get_rendered_control(ctxPtr, i)));
  }
  const [entries, bytes, maxBytes] = Geoscript.geoscript_repl_get_const_eval_cache_stats(ctxPtr);

  return {
    payload: {
      error: null,
      wasmTrap: false,
      asyncDepRetries: 0,
      durationMs,
      usedDepsBitmask,
      phases: {
        setup: tSetup - tStart,
        ambient: tAmbient - tSetup,
        evalWall: tEval - tAmbient,
        extract: performance.now() - tEval,
      },
      constEvalCache: { entries, bytes, maxBytes },
      vectorizeReports: JSON.parse(Geoscript.geoscript_repl_get_vectorize_reports(ctxPtr)),
      meshes,
      paths,
      lights,
      textures,
      gizmos,
      controls,
    },
    buffers,
  };
};

/** Deps a program needs but that weren't preloaded surface as a sentinel error; load them and
 *  re-run from scratch. `text_to_path` always takes this route since its args are runtime values. */
const runJobWithRetries = async (ctxPtr: number, job: WorkerRunJob) => {
  if (job.asyncDeps?.length) {
    await initAsyncDeps(depsOf(job.asyncDeps), {});
  }
  const tried = new Set<string>();
  for (let asyncDepRetries = 0; ; asyncDepRetries += 1) {
    const attempt = runAttempt(ctxPtr, job);
    const dep = attempt.payload.error ? parseUninitializedDep(attempt.payload.error) : null;
    // A dep that's still missing after its init is one this worker can't provide; surface the error.
    if (!dep || tried.has(dep.name)) {
      attempt.payload.asyncDepRetries = asyncDepRetries;
      return attempt;
    }
    tried.add(dep.name);
    await initAsyncDeps({ [dep.name]: true }, dep.args ? { [dep.name]: dep.args } : {});
  }
};

type EagerDeps = Pick<
  GeoscriptAsyncDeps,
  'meshopt' | 'cgal' | 'clipper2' | 'geodesics' | 'uv_unwrap' | 'uv_solvers'
>;

const init = async (urls: GeoscriptWorkerWasmURLs, eagerDeps?: EagerDeps) => {
  geoscriptReplWasmURL = urls.geoscriptRepl;
  setManifoldWasmURL(urls.manifold);
  setCGALWasmURL(urls.cgal);
  setClipper2WasmURL(urls.clipper2);
  setGeodesicsWasmURL(urls.geodesics);
  setUVUnwrapWasmURL(urls.uvUnwrap);
  setUVSolversWasmURL(urls.uvSolvers);
  setModelDataURLs(urls.modelData);

  const eagerInits: Promise<unknown>[] = [];
  if (eagerDeps?.meshopt) eagerInits.push(initMeshopt());
  if (eagerDeps?.cgal) {
    const p = initCGAL();
    if (p instanceof Promise) {
      eagerInits.push(p);
    }
  }
  if (eagerDeps?.clipper2) {
    const p = initClipper2();
    if (p instanceof Promise) {
      eagerInits.push(p);
    }
  }
  if (eagerDeps?.geodesics) {
    eagerInits.push(initGeodesics());
  }
  if (eagerDeps?.uv_unwrap) {
    eagerInits.push(initUVUnwrap());
  }
  if (eagerDeps?.uv_solvers) {
    eagerInits.push(initUVSolvers());
  }

  const [_manifold, repl] = await Promise.all([initManifoldWasm(), initGeoscript(), ...eagerInits]);
  return repl.geoscript_repl_init();
};

/** A prespawned worker (see `prespawnWorker.ts`) carries its init args in `name` and starts
 *  loading before the main thread's `init()` call, which then just adopts the in-flight promise. */
let initPromise: Promise<number> | null = null;
if (self.name) {
  const { urls, eagerDeps } = JSON.parse(self.name) as {
    urls: GeoscriptWorkerWasmURLs;
    eagerDeps?: EagerDeps;
  };
  initPromise = init(urls, eagerDeps);
  initPromise.catch(() => {});
}

const methods = {
  init: (urls: GeoscriptWorkerWasmURLs, eagerDeps?: EagerDeps) => (initPromise ??= init(urls, eagerDeps)),
  initAsyncDeps: async (
    deps: GeoscriptAsyncDeps,
    argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>>
  ) => {
    await initAsyncDeps(deps, argsByKey);
  },
  initAsyncDep: async (name: keyof GeoscriptAsyncDeps, args?: string[]) => {
    const argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>> = {};
    if (args?.length) {
      argsByKey[name] = args;
    }
    await initAsyncDeps({ [name]: true }, argsByKey);
  },
  clearConstEvalCache: (ctxPtr: number) => {
    Geoscript.geoscript_repl_clear_const_eval_cache(ctxPtr);
  },
  /** Everything a run needs in one message: no per-item round trips, and no queued eval can
   *  reset the ctx between eval and extraction. */
  runJob: async (ctxPtr: number, job: WorkerRunJob) => {
    const { payload, buffers } = await runJobWithRetries(ctxPtr, job);
    return Comlink.transfer(payload, buffers);
  },
  /** Run `jobs` back to back, streaming each result to `onResult` (a `Comlink.proxy`) without
   *  waiting for the main thread, so a busy main thread never starves the worker. */
  runJobs: async (
    ctxPtr: number,
    jobs: (WorkerRunJob & { id: string })[],
    onResult: (id: string, payload: WorkerRunPayload) => void
  ) => {
    const warm = new Set(jobs.flatMap(j => j.asyncDeps ?? []));
    if (warm.size) {
      void Promise.resolve(initAsyncDeps(depsOf([...warm]), {})).catch(() => {});
    }
    try {
      for (const job of jobs) {
        const { payload, buffers } = await runJobWithRetries(ctxPtr, job).catch(err => ({
          payload: emptyPayload(String(enrichWasmError(err)), isWasmTrap(err)),
          buffers: [] as Transferable[],
        }));
        void Promise.resolve(onResult(job.id, Comlink.transfer(payload, buffers))).catch(err =>
          console.error('[geoscriptWorker] onResult failed:', err)
        );
      }
    } finally {
      (onResult as unknown as Comlink.Remote<typeof onResult>)[Comlink.releaseProxy]();
    }
  },
  /** Eval-mode: root program's own top-level bindings as tagged-JSON (see `value_json.rs`). */
  getExportsJson: (ctxPtr: number, sampleCount: number): string =>
    Geoscript.geoscript_repl_get_exports_json(ctxPtr, sampleCount),
  /** Eval-mode: tagged-JSON value of the run's last top-level statement (`--expr` appends it). */
  getLastValueJson: (ctxPtr: number, sampleCount: number): string => {
    lastWasmPanic = null;
    try {
      return Geoscript.geoscript_repl_get_last_value_json(ctxPtr, sampleCount);
    } catch (err) {
      throw enrichWasmError(err);
    }
  },
  /** Eval-mode: drain `print()` output captured during the last run. */
  takePrints: (ctxPtr: number): string[] => Geoscript.geoscript_repl_take_prints(ctxPtr),
  /** Snapshot of every rendered mesh's UV-view data in ONE worker message: this method is
   *  synchronous, so a queued eval's reset can't interleave between per-mesh fetches. */
  getAllRenderedMeshUvData: (ctxPtr: number) => {
    const count = Geoscript.geoscript_repl_get_rendered_mesh_count(ctxPtr);
    const out: {
      verts: Float32Array;
      indices: Uint32Array;
      uvs: Float32Array | null;
      sourceModule: string | null;
      material: string;
    }[] = [];
    const buffers: ArrayBuffer[] = [];
    for (let i = 0; i < count; i += 1) {
      const verts = Geoscript.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, i);
      const indices = Geoscript.geoscript_repl_get_rendered_mesh_indices(ctxPtr, i);
      const uvs = Geoscript.geoscript_repl_get_rendered_mesh_uvs(ctxPtr, i) ?? null;
      out.push({
        verts,
        indices,
        uvs,
        sourceModule: Geoscript.geoscript_repl_get_rendered_mesh_source_module(ctxPtr, i) ?? null,
        material: Geoscript.geoscript_repl_get_rendered_mesh_material(ctxPtr, i),
      });
      buffers.push(verts.buffer as ArrayBuffer, indices.buffer as ArrayBuffer);
      if (uvs) buffers.push(uvs.buffer as ArrayBuffer);
    }
    return Comlink.transfer(out, buffers);
  },
  setMaterials: (ctxPtr: number, defaultMaterialID: string | null, availableMaterials: string[]) => {
    Geoscript.geoscript_set_default_material(ctxPtr, defaultMaterialID ?? undefined);
    Geoscript.geoscript_set_materials(ctxPtr, availableMaterials);
  },
  getPrelude: (kind: string) => Geoscript.geoscript_repl_get_prelude(kind),
  /**
   * Compute the convex hull of `verts` (flat xyz Float32Array, asset-local space) using
   * Manifold and return the resulting triangle mesh data.  Manifold and the geoscript wasm
   * are loaded together at worker init, so this is safe to call any time after `init()`
   * resolves — independent of any geoscript context.
   */
  computeConvexHull: (verts: Float32Array): { verts: Float32Array; indices: Uint32Array } => {
    const out = compute_convex_hull_mesh(verts);
    return Comlink.transfer(out, [out.verts.buffer, out.indices.buffer]);
  },
};

export type GeoscriptWorkerMethods = typeof methods;

Comlink.expose(methods);
