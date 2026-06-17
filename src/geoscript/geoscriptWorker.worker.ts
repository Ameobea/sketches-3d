import * as Comlink from 'comlink';

import { compute_convex_hull_mesh, initManifoldWasm, setManifoldWasmURL } from './manifold';
import type { Light } from 'src/viz/scenes/geoscriptPlayground/lights';
import type { GizmoValuesByModule } from './runner/types';
import * as Geoscript from 'src/viz/wasmComp/geoscript_repl';

/** Raw shape of `geoscript_repl_get_rendered_gizmo`'s JSON (snake_case from Rust). */
interface RawRenderedGizmo {
  source_module: string | null;
  handle_id: string;
  kind: 'vec3' | 'transform';
  origin: [number, number, number];
  value: number[];
  absolute: boolean;
}
import { initGeodesics, setGeodesicsWasmURL } from './geodesics';
import { initCGAL, setCGALWasmURL } from 'src/viz/wasm/cgal/cgal';
import { initClipper2, setClipper2WasmURL } from 'src/viz/wasm/clipper2/clipper2';
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

export interface GeoscriptAsyncDeps {
  geodesics?: boolean;
  cgal?: boolean;
  text_to_path?: boolean;
  clipper2?: boolean;
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
    eagerDeps?: { cgal?: boolean; clipper2?: boolean; geodesics?: boolean }
  ) => {
    geoscriptReplWasmURL = urls.geoscriptRepl;
    setManifoldWasmURL(urls.manifold);
    setCGALWasmURL(urls.cgal);
    setClipper2WasmURL(urls.clipper2);
    setGeodesicsWasmURL(urls.geodesics);

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
  setModuleSources: (ctxPtr: number, modules: Record<string, string>) => {
    const names = Object.keys(modules);
    const sources = Object.values(modules);
    Geoscript.geoscript_repl_set_module_sources(ctxPtr, names, sources);
  },
  /**
   * Install an ambient scope built by sequentially evaluating each provided source
   * (typically `[prelude_src, globals_src]`). The resulting scope is cloned as the
   * base for every subsequent module evaluation. Pass an empty array to reset.
   * Throws if any source fails to evaluate.
   */
  setAmbientScope: (ctxPtr: number, sources: string[]) => {
    if (sources.length === 0) {
      Geoscript.geoscript_repl_clear_ambient_scope(ctxPtr);
      return;
    }
    Geoscript.geoscript_repl_set_ambient_scope_from_sources(ctxPtr, sources);
  },
  eval: async (ctxPtr: number, code: string, includePrelude: boolean) => {
    Geoscript.geoscript_repl_parse_program(ctxPtr, code, includePrelude);
    if (Geoscript.geoscript_repl_has_err(ctxPtr)) {
      return { durationMs: 0, usedDepsBitmask: 0 };
    }

    const start = performance.now();
    Geoscript.geoscript_repl_eval(ctxPtr);
    const durationMs = performance.now() - start;
    const usedDepsBitmask = Geoscript.geoscript_repl_get_used_async_deps(ctxPtr);
    return { durationMs, usedDepsBitmask };
  },
  getErr: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_get_err(ctxPtr);
  },
  getRenderedMeshCount: (ctxPtr: number) => {
    return Geoscript.geoscript_repl_get_rendered_mesh_count(ctxPtr);
  },
  getRenderedMeshIndicesWithMaterial: (ctxPtr: number, materialId: string) => {
    return Geoscript.geoscript_repl_get_rendered_mesh_indices_with_material(ctxPtr, materialId);
  },
  getRenderedMesh: (ctxPtr: number, meshIx: number) => {
    const transform = Geoscript.geoscript_repl_get_rendered_mesh_transform(ctxPtr, meshIx);
    const verts = Geoscript.geoscript_repl_get_rendered_mesh_vertices(ctxPtr, meshIx);
    const indices = Geoscript.geoscript_repl_get_rendered_mesh_indices(ctxPtr, meshIx);
    const normals = Geoscript.geoscript_repl_get_rendered_mesh_normals(ctxPtr, meshIx);
    const material = Geoscript.geoscript_repl_get_rendered_mesh_material(ctxPtr, meshIx);
    const sourceModule = Geoscript.geoscript_repl_get_rendered_mesh_source_module(ctxPtr, meshIx);
    const meshId = Geoscript.geoscript_repl_get_rendered_mesh_id(ctxPtr, meshIx);

    return Comlink.transfer(
      { verts, indices, normals, transform, material, sourceModule, meshId },
      filterNils([verts.buffer, indices.buffer, normals?.buffer])
    );
  },
  getRenderedPathCount: (ctxPtr: number) => {
    return Geoscript.geoscript_get_rendered_path_count(ctxPtr);
  },
  getRenderedPath: (ctxPtr: number, pathIx: number) => {
    const verts = Geoscript.geoscript_get_rendered_path(ctxPtr, pathIx);
    const pathId = Geoscript.geoscript_get_rendered_path_id(ctxPtr, pathIx);
    return Comlink.transfer({ verts, pathId }, [verts.buffer]);
  },
  getRenderedLightCount: (ctxPtr: number) => {
    return Geoscript.geoscript_get_rendered_light_count(ctxPtr);
  },
  getRenderedLight: (ctxPtr: number, lightIx: number): { light: Light; lightId: number } => {
    const light = JSON.parse(Geoscript.geoscript_get_rendered_light(ctxPtr, lightIx));
    const lightId = Geoscript.geoscript_get_rendered_light_id(ctxPtr, lightIx);
    return Comlink.transfer({ light, lightId }, []);
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
  setMaterials: (ctxPtr: number, defaultMaterialID: string | null, availableMaterials: string[]) => {
    Geoscript.geoscript_set_default_material(ctxPtr, defaultMaterialID ?? undefined);
    Geoscript.geoscript_set_materials(ctxPtr, availableMaterials);
  },
  getPrelude: () => Geoscript.geoscript_repl_get_prelude(),
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
