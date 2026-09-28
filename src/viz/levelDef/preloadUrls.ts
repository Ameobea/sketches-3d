import { WASM_ASSET_URLS } from 'src/viz/wasmComp/wasmAssetURLs';
import type { GeoscriptAssetMeta, LevelDef } from './types';

export interface LevelDefEagerDeps {
  cgal: boolean;
  clipper2: boolean;
  geodesics: boolean;
  uv_unwrap: boolean;
  uv_solvers: boolean;
}

export const collectLevelDefEagerDeps = (levelDef: LevelDef | null): LevelDefEagerDeps => {
  const out: LevelDefEagerDeps = {
    cgal: false,
    clipper2: false,
    geodesics: false,
    uv_unwrap: false,
    uv_solvers: false,
  };
  if (!levelDef) {
    return out;
  }
  for (const asset of Object.values(levelDef.assets ?? {})) {
    const meta = (asset as { _meta?: GeoscriptAssetMeta })._meta;
    if (!meta?.asyncDeps) {
      continue;
    }
    for (const d of meta.asyncDeps) {
      if (d === 'cgal' || d === 'clipper2' || d === 'geodesics' || d === 'uv_unwrap' || d === 'uv_solvers') {
        out[d] = true;
      }
    }
  }
  return out;
};

/**
 * Comma-joined eager dep names for the `geoscript-eager-deps` meta the client `init` hook reads to
 * prespawn the worker; `undefined` when no asset needs the executor (mirrors `needsExecutor`).
 */
export const getSceneEagerDepNames = (levelDef: LevelDef | null): string | undefined => {
  const needsExecutor = Object.values(levelDef?.assets ?? {}).some(
    a => a.type !== 'gltf' || a.colliderShape === 'convexHull'
  );
  if (!needsExecutor) {
    return undefined;
  }
  return Object.entries(collectLevelDefEagerDeps(levelDef))
    .filter(([, on]) => on)
    .map(([name]) => name)
    .join(',');
};

/** Only wasm the main thread itself fetches. The geoscript worker fetches its own wasm; a document
 *  preload of the same URL that's still in flight when the worker asks is a second download. */
export const getScenePreloadUrls = (): string[] => [WASM_ASSET_URLS.ammo, WASM_ASSET_URLS.flightRecorder];
