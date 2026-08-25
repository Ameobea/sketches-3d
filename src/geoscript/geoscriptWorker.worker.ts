import * as Comlink from 'comlink';

import { compute_convex_hull_mesh, initManifoldWasm, setManifoldWasmURL } from './manifold';
import type { Light } from 'src/geotoy/modes/mesh/lights';
import type {
  ConstEvalCacheStats,
  GizmoValuesByModule,
  TextureParamsEntry,
  VectorizeFlags,
  VectorizeReport,
} from './runner/types';
import * as Geoscript from 'src/viz/wasmComp/geoscript_repl';

/** Raw shape of `geoscript_repl_get_rendered_gizmo`'s JSON (snake_case from Rust). */
interface RawRenderedGizmo {
  source_module: string | null;
  handle_id: string;
  kind: 'vec3' | 'transform';
  origin: [number, number, number];
  value: number[];
  absolute: boolean;
  axes: [boolean, boolean, boolean];
  ghost: boolean | null;
}

/** Raw shape of `geoscript_repl_get_rendered_control`'s JSON (snake_case from Rust). */
interface RawRenderedControl {
  source_module: string | null;
  handle_id: string;
  kind: 'float' | 'int' | 'bool' | 'color' | 'select' | 'spline' | 'ramp' | 'image_levels' | 'uv_params';
  label: string | null;
  value: number[];
  str_value: string | null;
  min: number | null;
  max: number | null;
  step: number | null;
  style: string | null;
  options: string[];
  stats: number[] | null;
  has_override: boolean;
}
import { initGeodesics, setGeodesicsWasmURL } from './geodesics';
import { initCGAL, setCGALWasmURL } from 'src/viz/wasm/cgal/cgal';
import { initClipper2, setClipper2WasmURL } from 'src/viz/wasm/clipper2/clipper2';
import { initUVUnwrap, setUVUnwrapWasmURL } from './uvUnwrap';
import { initUVSolvers, setUVSolversWasmURL } from './uvSolvers';
import { initImageData } from './imageData';
import { initModelData, setModelDataURLs } from './modelData';
import { textToSvg } from './text_to_path';
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
  await Geoscript.default(fetch(geoscriptReplWasmURL));
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

const methods = {
  init: async (
    urls: GeoscriptWorkerWasmURLs,
    eagerDeps?: {
      cgal?: boolean;
      clipper2?: boolean;
      geodesics?: boolean;
      uv_unwrap?: boolean;
      uv_solvers?: boolean;
    }
  ) => {
    geoscriptReplWasmURL = urls.geoscriptRepl;
    setManifoldWasmURL(urls.manifold);
    setCGALWasmURL(urls.cgal);
    setClipper2WasmURL(urls.clipper2);
    setGeodesicsWasmURL(urls.geodesics);
    setUVUnwrapWasmURL(urls.uvUnwrap);
    setUVSolversWasmURL(urls.uvSolvers);
    setModelDataURLs(urls.modelData);

    const eagerInits: Promise<unknown>[] = [];
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
  },
  reset: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_reset(ctxPtr);
  },
  initAsyncDeps: async (
    deps: GeoscriptAsyncDeps,
    argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>>
  ) => {
    await initAsyncDeps(deps, argsByKey);
  },
  initAsyncDep: async (name: keyof GeoscriptAsyncDeps, args?: string[]) => {
    const deps: GeoscriptAsyncDeps = { [name]: true };
    const argsByKey: Partial<Record<keyof GeoscriptAsyncDeps, string[]>> = {};
    if (args?.length) {
      argsByKey[name] = args;
    }
    await initAsyncDeps(deps, argsByKey);
  },
  clearConstEvalCache: (ctxPtr: number) => {
    Geoscript.geoscript_repl_clear_const_eval_cache(ctxPtr);
  },
  getConstEvalCacheStats: (ctxPtr: number): ConstEvalCacheStats => {
    const [entries, bytes, maxBytes] = Geoscript.geoscript_repl_get_const_eval_cache_stats(ctxPtr);
    return { entries, bytes, maxBytes };
  },
  setModuleSources: (
    ctxPtr: number,
    modules: Record<string, string>,
    modulePreludes?: Record<string, string>
  ) => {
    const names = Object.keys(modules);
    const sources = names.map(name => {
      const kind = modulePreludes?.[name];
      return kind ? `${Geoscript.geoscript_repl_get_prelude(kind)}\n${modules[name]}` : modules[name];
    });
    Geoscript.geoscript_repl_set_module_sources(ctxPtr, names, sources);
  },
  /**
   * Install an ambient scope built by sequentially evaluating each provided source
   * (typically `[prelude_src, globals_src]`). The resulting scope is cloned as the
   * base for every subsequent module evaluation. Pass an empty array to reset.
   * Throws if any source fails to evaluate.
   */
  setAmbientScope: (ctxPtr: number, sources: string[], rootModuleName?: string) => {
    if (sources.length === 0) {
      Geoscript.geoscript_repl_clear_ambient_scope(ctxPtr);
      return;
    }
    lastWasmPanic = null;
    try {
      Geoscript.geoscript_repl_set_ambient_scope_from_sources(ctxPtr, sources, rootModuleName);
    } catch (err) {
      throw enrichWasmError(err);
    }
  },
  /**
   * Per-tab ambient scopes for multi-tab runs; one (tabId, preludeKind, globalsSource)
   * triple per run-set tab, active tab last. Preludes are resolved wasm-side from the kind.
   */
  setTabAmbientScopes: (
    ctxPtr: number,
    tabIds: string[],
    preludeKinds: string[],
    globalsSources: string[]
  ) => {
    lastWasmPanic = null;
    try {
      Geoscript.geoscript_repl_set_tab_ambient_scopes(ctxPtr, tabIds, preludeKinds, globalsSources);
    } catch (err) {
      throw enrichWasmError(err);
    }
  },
  eval: async (ctxPtr: number, code: string, preludeKind: string | undefined, rootModuleName?: string) => {
    lastWasmPanic = null;
    try {
      Geoscript.geoscript_repl_parse_program(ctxPtr, code, preludeKind);
      if (Geoscript.geoscript_repl_has_err(ctxPtr)) {
        return { durationMs: 0, usedDepsBitmask: 0 };
      }

      const start = performance.now();
      Geoscript.geoscript_repl_eval(ctxPtr, rootModuleName);
      const durationMs = performance.now() - start;
      const usedDepsBitmask = Geoscript.geoscript_repl_get_used_async_deps(ctxPtr);
      return { durationMs, usedDepsBitmask };
    } catch (err) {
      throw enrichWasmError(err);
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
  getErr: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_get_err(ctxPtr);
  },
  getRenderedMeshCount: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_get_rendered_mesh_count(ctxPtr);
  },
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
  getRenderedMesh: (ctxPtr: number, meshIx: number) => {
    const transform = Geoscript.geoscript_repl_get_rendered_mesh_transform(ctxPtr, meshIx);
    const verts = Geoscript.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, meshIx);
    const indices = Geoscript.geoscript_repl_get_rendered_mesh_indices(ctxPtr, meshIx);
    const normals = Geoscript.geoscript_repl_get_rendered_mesh_normals(ctxPtr, meshIx);
    const uvs = Geoscript.geoscript_repl_get_rendered_mesh_uvs(ctxPtr, meshIx);
    const tangents = Geoscript.geoscript_repl_get_rendered_mesh_tangents(ctxPtr, meshIx);
    const material = Geoscript.geoscript_repl_get_rendered_mesh_material(ctxPtr, meshIx);
    const sourceModule = Geoscript.geoscript_repl_get_rendered_mesh_source_module(ctxPtr, meshIx);
    const meshId = Geoscript.geoscript_repl_get_rendered_mesh_id(ctxPtr, meshIx);

    return Comlink.transfer(
      { verts, indices, normals, uvs, tangents, transform, material, sourceModule, meshId },
      filterNils([verts.buffer, indices.buffer, normals?.buffer, uvs?.buffer, tangents?.buffer])
    );
  },
  getRenderedPathCount: (ctxPtr: number) => {
    return Geoscript.geoscript_get_rendered_path_count(ctxPtr);
  },
  getRenderedPath: (ctxPtr: number, pathIx: number) => {
    const verts = Geoscript.geoscript_get_rendered_path(ctxPtr, pathIx);
    const pathId = Geoscript.geoscript_get_rendered_path_id(ctxPtr, pathIx);
    const sourceModule = Geoscript.geoscript_get_rendered_path_source_module(ctxPtr, pathIx);
    return Comlink.transfer({ verts, pathId, sourceModule }, [verts.buffer]);
  },
  getRenderedLightCount: (ctxPtr: number) => {
    return Geoscript.geoscript_get_rendered_light_count(ctxPtr);
  },
  getRenderedLight: (
    ctxPtr: number,
    lightIx: number
  ): { light: Light; lightId: number; sourceModule: string } => {
    const light = JSON.parse(Geoscript.geoscript_get_rendered_light(ctxPtr, lightIx));
    const lightId = Geoscript.geoscript_get_rendered_light_id(ctxPtr, lightIx);
    const sourceModule = Geoscript.geoscript_get_rendered_light_source_module(ctxPtr, lightIx);
    return Comlink.transfer({ light, lightId, sourceModule }, []);
  },
  getRenderedTextureCount: (ctxPtr: number) => Geoscript.geoscript_get_rendered_texture_count(ctxPtr),
  getVectorizeReports: (ctxPtr: number): VectorizeReport[] =>
    JSON.parse(Geoscript.geoscript_repl_get_vectorize_reports(ctxPtr)),
  setVectorizeFlags: (ctxPtr: number, flags: VectorizeFlags) => {
    Geoscript.geoscript_repl_set_no_vectorize(ctxPtr, flags.disabled);
    Geoscript.geoscript_repl_set_verify(ctxPtr, flags.verify);
    Geoscript.geoscript_repl_set_vectorize_profile(ctxPtr, flags.profile);
  },
  getRenderedTexture: (ctxPtr: number, texIx: number) => {
    const [width, height, channels] = Geoscript.geoscript_get_rendered_texture_dims(ctxPtr, texIx);
    /** 1 for plain outputs; stacks concatenate their slices in `pixels`. */
    const layers = Geoscript.geoscript_get_rendered_texture_layers(ctxPtr, texIx);
    const name = Geoscript.geoscript_get_rendered_texture_name(ctxPtr, texIx);
    /** Empty string when no usage was declared. */
    const usage = Geoscript.geoscript_get_rendered_texture_usage(ctxPtr, texIx);
    const wrap = Geoscript.geoscript_get_rendered_texture_wrap(ctxPtr, texIx);
    const sourceModule = Geoscript.geoscript_get_rendered_texture_source_module(ctxPtr, texIx);
    const textureId = Geoscript.geoscript_get_rendered_texture_id(ctxPtr, texIx);
    const stats = Geoscript.geoscript_get_rendered_texture_stats(ctxPtr, texIx);
    /** Empty strings mean unset. */
    const [minFilter, magFilter, format] = Geoscript.geoscript_get_rendered_texture_gpu_params(ctxPtr, texIx);
    /** SIMD-encoded in wasm for u8 materialization formats; empty for float formats. */
    const encodedRaw = Geoscript.geoscript_encode_rendered_texture_pixels(ctxPtr, texIx);
    const encoded = encodedRaw.length ? encodedRaw : undefined;
    // must mirror the wasm export's unset-format resolution
    const encodedFormat = encoded ? format || 'rgba8' : undefined;
    /** See `GeneratedTexture.rgba`. */
    const rgba =
      channels === 3 ? Geoscript.geoscript_get_rendered_texture_pixels_rgba(ctxPtr, texIx) : undefined;
    /** A 3-channel output's raw pixels have no consumer: the 2D preview reads `rgba` and
     *  every materialization format reads `encoded` or `rgba` — except r32f/rg32f, which
     *  take channel 0/1 off the raw interleave. Skipping it is the single biggest chunk of
     *  the export for rgb stacks. */
    const rawFloatFormat = format === 'r32f' || format === 'rg32f';
    const pixels =
      channels === 3 && !rawFloatFormat
        ? undefined
        : Geoscript.geoscript_get_rendered_texture_pixels(ctxPtr, texIx);
    return Comlink.transfer(
      {
        width,
        height,
        channels,
        layers,
        pixels,
        encoded,
        encodedFormat,
        rgba,
        name,
        usage,
        wrap,
        sourceModule,
        textureId,
        minFilter,
        magFilter,
        format,
        stats,
      },
      filterNils([pixels?.buffer, encoded?.buffer, rgba?.buffer, stats.buffer])
    );
  },
  setTextureParams: (ctxPtr: number, entries: TextureParamsEntry[]) => {
    Geoscript.geoscript_repl_set_texture_params(
      ctxPtr,
      entries.map(e => e.tabId),
      entries.map(e => e.name),
      entries.map(e => e.minFilter ?? ''),
      entries.map(e => e.magFilter ?? ''),
      entries.map(e => e.format ?? '')
    );
  },
  setGizmoValues: (ctxPtr: number, valuesByModule: GizmoValuesByModule) => {
    const modules: string[] = [];
    const handles: string[] = [];
    const valuesJson: string[] = [];
    for (const [mod, handleMap] of Object.entries(valuesByModule)) {
      for (const [handle, v] of Object.entries(handleMap)) {
        modules.push(mod);
        handles.push(handle);
        valuesJson.push(JSON.stringify(v));
      }
    }
    Geoscript.geoscript_repl_set_gizmo_values(ctxPtr, modules, handles, valuesJson);
  },
  getRenderedGizmoCount: (ctxPtr: number) => Geoscript.geoscript_repl_get_rendered_gizmo_count(ctxPtr),
  getRenderedGizmo: (ctxPtr: number, ix: number): RawRenderedGizmo =>
    JSON.parse(Geoscript.geoscript_repl_get_rendered_gizmo(ctxPtr, ix)),
  getRenderedControlCount: (ctxPtr: number) => Geoscript.geoscript_repl_get_rendered_control_count(ctxPtr),
  getRenderedControl: (ctxPtr: number, ix: number): RawRenderedControl =>
    JSON.parse(Geoscript.geoscript_repl_get_rendered_control(ctxPtr, ix)),
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
