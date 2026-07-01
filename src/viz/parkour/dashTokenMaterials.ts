import type * as THREE from 'three';

import { buildMaterial } from 'src/viz/materials';
import { normalizeRawDefColors, type LibraryMaterialFile, type MaterialDef } from 'src/viz/levelDef/types';
import { TextureFetchPool } from 'src/viz/levelDef/texturePool';
import dashTokenCoreLib from 'src/assets/materials/parkour/dash_token_core.json';
import dashTokenRingLib from 'src/assets/materials/parkour/dash_token_ring.json';

export interface DashTokenMaterials {
  core: THREE.Material;
  ring: THREE.Material;
}

// Textures are cached for the session (they outlive material disposal, and `Material.dispose()`
// doesn't dispose its textures); materials are built fresh per call so a disposed material from a
// torn-down scene is never reused.
const texturePool = new TextureFetchPool();
const textureCache = new Map<LibraryMaterialFile, Promise<Map<string, THREE.Texture>>>();

const loadTextures = (lib: LibraryMaterialFile): Promise<Map<string, THREE.Texture>> => {
  let cached = textureCache.get(lib);
  if (!cached) {
    cached = (async () => {
      const texMap = new Map<string, THREE.Texture>();
      await Promise.all(
        Object.entries(lib.textures ?? {}).map(async ([name, def]) => {
          texMap.set(name, await texturePool.load(def, name));
        })
      );
      return texMap;
    })();
    textureCache.set(lib, cached);
  }
  return cached;
};

/** Build a self-contained library material file (no `extends`/shader-file refs) into a THREE material. */
const buildLibraryMaterial = async (lib: LibraryMaterialFile): Promise<THREE.Material> =>
  buildMaterial(normalizeRawDefColors(lib.material) as MaterialDef, await loadTextures(lib));

/**
 * Default dash-token core/ring materials, built from the shared library items
 * (`__ASSETS__/materials/parkour/dash_token_{core,ring}`) — the green-mosaic + gold pair the
 * original Blender parkour levels use.
 */
export const buildDefaultDashTokenMaterials = (): Promise<DashTokenMaterials> =>
  Promise.all([
    buildLibraryMaterial(dashTokenCoreLib as LibraryMaterialFile),
    buildLibraryMaterial(dashTokenRingLib as LibraryMaterialFile),
  ]).then(([core, ring]) => ({ core, ring }));
