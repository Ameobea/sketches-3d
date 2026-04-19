import ammoWasmURL from 'src/ammojs/ammo.wasm.wasm?url';
import geodesicsWasmURL from 'src/geodesics/geodesics.wasm?url';
import cgalWasmURL from 'src/viz/wasm/cgal/index.wasm?url';
import clipper2WasmURL from 'src/viz/wasm/clipper2/clipper2z.wasm?url';
import flightRecorderWasmURL from 'src/viz/wasmComp/flight_recorder.wasm?url';
import geoscriptReplWasmURL from 'src/viz/wasmComp/geoscript_repl_bg.wasm?url';
import manifoldWasmURL from 'manifold-3d/manifold.wasm?url';

import type { GeoscriptAssetMeta, LevelDef } from './types';

/**
 * Returns the hashed URLs of wasm assets the scene will fetch during load.
 * Consumed by `[scene]/+page.svelte` to emit `<link rel="preload">` tags in
 * `<head>`, kicking off fetches before any JS runs and collapsing the
 * otherwise-serial worker-boot → dep-load waterfall.
 *
 * Pass `null` for non-`useSceneDef` routes to get just the shared core
 * (ammo + flight_recorder), which `initViz` always pre-fetches regardless.
 */
export const getScenePreloadUrls = (levelDef: LevelDef | null): string[] => {
  // ammo + flight_recorder are pre-fetched unconditionally by `initViz`, so they
  // benefit every Viz-backed route, not just useSceneDef ones.
  const urls: string[] = [ammoWasmURL, flightRecorderWasmURL];
  if (!levelDef) {
    return urls;
  }

  urls.push(geoscriptReplWasmURL, manifoldWasmURL);

  const asyncDeps = new Set<string>();
  for (const asset of Object.values(levelDef.assets ?? {})) {
    const meta = (asset as { _meta?: GeoscriptAssetMeta })._meta;
    if (meta?.asyncDeps) {
      for (const d of meta.asyncDeps) {
        asyncDeps.add(d);
      }
    }
  }

  if (asyncDeps.has('cgal')) {
    urls.push(cgalWasmURL);
  }
  if (asyncDeps.has('clipper2')) {
    urls.push(clipper2WasmURL);
  }
  if (asyncDeps.has('geodesics')) {
    urls.push(geodesicsWasmURL);
  }

  return urls;
};
