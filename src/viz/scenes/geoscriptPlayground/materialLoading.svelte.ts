import type * as THREE from 'three';
import { getMultipleTextures, type TextureID } from 'src/geoscript/geotoyAPIClient';
import { buildMaterial, FallbackMat, LoadedTextures, type MaterialDef } from 'src/geoscript/materials';
import type { MatEntry } from 'src/geoscript/runner/types';
import type { Viz } from 'src/viz';
import { CustomBasicShaderMaterial } from 'src/viz/shaders/customBasicShader';
import { CustomShaderMaterial } from 'src/viz/shaders/customShader';
import { Textures } from './materialEditor/state.svelte';
import { loadTexture } from 'src/viz/textureLoading';

export const fetchAndSetTextures = async (loader: THREE.ImageBitmapLoader, textureIDs: TextureID[]) => {
  const missingTextureIDs = textureIDs.filter(id => !(id in LoadedTextures));
  if (missingTextureIDs.length === 0) {
    return;
  }

  const resolvers: Map<TextureID, (tex: THREE.Texture) => void> = new Map();
  for (const id of missingTextureIDs) {
    if (LoadedTextures.has(id)) {
      continue;
    }
    const p = new Promise<THREE.Texture>(resolve => {
      resolvers.set(id, resolve);
    });
    LoadedTextures.set(id, p);
  }

  const adminToken = new URLSearchParams(window.location.search).get('admin_token') ?? undefined;
  await getMultipleTextures(missingTextureIDs, undefined, adminToken).then(textures => {
    const allTextures = { ...Textures.textures };
    textures.forEach(tex => {
      allTextures[tex.id] = tex;
      const resolver = resolvers.get(tex.id);
      if (resolver) {
        const threeTexP = loadTexture(loader, tex.url);
        LoadedTextures.set(tex.id, threeTexP);
        threeTexP.then(threeTex => {
          resolver(threeTex);
          LoadedTextures.set(tex.id, threeTex);
        });
      }
    });
    Textures.textures = { ...Textures.textures, ...allTextures };
  });
};

export const buildCustomMaterials = (
  loader: THREE.ImageBitmapLoader,
  materialDefinitions: Record<string, MaterialDef>,
  viz: Viz,
  /** Called with the combined failure message, or `null` once all builds succeed.
   *  Failed materials fall back to `FallbackMat`. */
  onError?: (msg: string | null) => void
) => {
  const builtMats: Record<string, MatEntry> = {};
  const errorsById: Record<string, string> = {};
  const report = () => {
    if (!onError) {
      return;
    }
    const msgs = Object.values(errorsById);
    onError(msgs.length ? msgs.join('\n\n') : null);
  };
  const recordError = (def: MaterialDef, e: unknown) => {
    const name = def.name ?? 'material';
    errorsById[name] = `Material "${name}": ${e instanceof Error ? e.message : String(e)}`;
  };

  // TODO: needs hashing to avoid re-building materials that haven't changed
  for (const [id, def] of Object.entries(materialDefinitions)) {
    let matMaybeP: Promise<THREE.Material> | THREE.Material;
    try {
      matMaybeP = buildMaterial(loader, def as MaterialDef, id);
    } catch (e) {
      recordError(def as MaterialDef, e);
      matMaybeP = FallbackMat;
    }
    const entry: MatEntry = {
      promise: matMaybeP instanceof Promise ? matMaybeP : Promise.resolve(matMaybeP),
      resolved: matMaybeP instanceof Promise ? null : matMaybeP,
    };

    const maybeRegisterBeforeRenderCb = (mat: THREE.Material) => {
      if (!(mat instanceof CustomShaderMaterial || mat instanceof CustomBasicShaderMaterial)) {
        return;
      }
      if (def.type !== 'customShader' || !def.shaders) {
        return;
      }

      if (
        def.shaders.colorShader ||
        def.shaders.iridescenceShader ||
        def.shaders.metalnessShader ||
        def.shaders.roughnessShader
      ) {
        const beforeRenderCb = (curTimeSeconds: number) => mat.setCurTimeSeconds(curTimeSeconds);
        viz.registerBeforeRenderCb(beforeRenderCb);
        entry.beforeRenderCb = beforeRenderCb;
      }
    };

    if (matMaybeP instanceof Promise) {
      // Fall back on async failures too, so they don't surface as unhandled rejections.
      const safeP = matMaybeP.catch(e => {
        recordError(def as MaterialDef, e);
        report();
        return FallbackMat as THREE.Material;
      });
      entry.promise = safeP;
      safeP.then(mat => {
        maybeRegisterBeforeRenderCb(mat);
        entry.resolved = mat;
      });
    } else {
      maybeRegisterBeforeRenderCb(matMaybeP);
    }
    builtMats[id] = entry;
  }
  report();
  return builtMats;
};

export const getReferencedTextureIDs = (materials: Record<string, MaterialDef>): TextureID[] => {
  const textureIDs: TextureID[] = [];
  for (const mat of Object.values(materials)) {
    if (mat.type !== 'customShader' || !mat.props) {
      continue;
    }
    const p = mat.props;
    for (const handle of [
      p.map,
      p.normalMap,
      p.roughnessMap,
      p.metalnessMap,
      p.clearcoatNormalMap,
      p.pomHeightMap,
    ]) {
      if (handle != null) {
        textureIDs.push(Number(handle));
      }
    }
  }
  return textureIDs;
};
