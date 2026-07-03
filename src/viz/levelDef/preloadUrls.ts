import { WASM_ASSET_URLS } from 'src/viz/wasmComp/wasmAssetURLs';
import type { GeoscriptAssetMeta, LevelDef } from './types';

export interface LevelDefEagerDeps {
  cgal: boolean;
  clipper2: boolean;
  geodesics: boolean;
  uv_unwrap: boolean;
}

export const collectLevelDefEagerDeps = (levelDef: LevelDef | null): LevelDefEagerDeps => {
  const out: LevelDefEagerDeps = { cgal: false, clipper2: false, geodesics: false, uv_unwrap: false };
  if (!levelDef) {
    return out;
  }
  for (const asset of Object.values(levelDef.assets ?? {})) {
    const meta = (asset as { _meta?: GeoscriptAssetMeta })._meta;
    if (!meta?.asyncDeps) {
      continue;
    }
    for (const d of meta.asyncDeps) {
      if (d === 'cgal' || d === 'clipper2' || d === 'geodesics' || d === 'uv_unwrap') {
        out[d] = true;
      }
    }
  }
  return out;
};

export const getScenePreloadUrls = (levelDef: LevelDef | null): string[] => {
  const urls: string[] = [WASM_ASSET_URLS.ammo, WASM_ASSET_URLS.flightRecorder];
  if (!levelDef) {
    return urls;
  }

  urls.push(WASM_ASSET_URLS.geoscriptRepl, WASM_ASSET_URLS.manifold);

  const eager = collectLevelDefEagerDeps(levelDef);
  if (eager.cgal) {
    urls.push(WASM_ASSET_URLS.cgal);
  }
  if (eager.clipper2) {
    urls.push(WASM_ASSET_URLS.clipper2);
  }
  if (eager.geodesics) {
    urls.push(WASM_ASSET_URLS.geodesics);
  }
  if (eager.uv_unwrap) {
    urls.push(WASM_ASSET_URLS.uvUnwrap);
  }

  return urls;
};
