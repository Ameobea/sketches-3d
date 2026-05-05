import { WASM_ASSET_URLS } from 'src/viz/wasmComp/wasmAssetURLs';
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
  const urls: string[] = [WASM_ASSET_URLS.ammo, WASM_ASSET_URLS.flightRecorder];
  if (!levelDef) {
    return urls;
  }

  urls.push(WASM_ASSET_URLS.geoscriptRepl, WASM_ASSET_URLS.manifold);

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
    urls.push(WASM_ASSET_URLS.cgal);
  }
  if (asyncDeps.has('clipper2')) {
    urls.push(WASM_ASSET_URLS.clipper2);
  }
  if (asyncDeps.has('geodesics')) {
    urls.push(WASM_ASSET_URLS.geodesics);
  }

  return urls;
};
