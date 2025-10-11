import { getMultipleTextures, type TextureID } from 'src/geoscript/geotoyAPIClient';
import { buildMaterial, type MaterialDef } from 'src/geoscript/materials';
import type { MatEntry } from 'src/geoscript/runner/types';
import type { Viz } from 'src/viz';
import { CustomBasicShaderMaterial } from 'src/viz/shaders/customBasicShader';
import { CustomShaderMaterial } from 'src/viz/shaders/customShader';
import { Textures } from './materialEditor/state.svelte';

export const fetchAndSetTextures = async (textureIDs: TextureID[]) => {
  if (textureIDs.length === 0) {
    return;
  }

  const adminToken = new URLSearchParams(window.location.search).get('admin_token') ?? undefined;
  await getMultipleTextures(textureIDs, undefined, adminToken).then(textures => {
    const allTextures = { ...Textures.textures };
    for (const texture of textures) {
      allTextures[texture.id] = texture;
    }
    Textures.textures = allTextures;
  });
};

export const buildCustomMaterials = (
  loader: THREE.ImageBitmapLoader,
  materialDefinitions: Record<string, MaterialDef>,
  viz: Viz
) => {
  const builtMats: Record<string, MatEntry> = {};

  // TODO: needs hashing to avoid re-building materials that haven't changed
  for (const [id, def] of Object.entries(materialDefinitions)) {
    const matMaybeP = buildMaterial(loader, def as MaterialDef, id);
    const entry: MatEntry = {
      promise: matMaybeP instanceof Promise ? matMaybeP : Promise.resolve(matMaybeP),
      resolved: matMaybeP instanceof Promise ? null : matMaybeP,
    };

    const maybeRegisterBeforeRenderCb = (mat: THREE.Material) => {
      if (
        !(mat instanceof CustomShaderMaterial || mat instanceof CustomBasicShaderMaterial) ||
        !def.shaders
      ) {
        return;
      }

      if (
        def.shaders.color ||
        (def.type === 'physical' &&
          (def.shaders.iridescence || def.shaders.metalness || def.shaders.roughness))
      ) {
        const beforeRenderCb = (curTimeSeconds: number) => mat.setCurTimeSeconds(curTimeSeconds);
        viz.registerBeforeRenderCb(beforeRenderCb);
        entry.beforeRenderCb = beforeRenderCb;
      }
    };

    if (matMaybeP instanceof Promise) {
      matMaybeP.then(mat => {
        maybeRegisterBeforeRenderCb(mat);
        entry.resolved = mat;
      });
    } else {
      maybeRegisterBeforeRenderCb(matMaybeP);
    }
    builtMats[id] = entry;
  }
  return builtMats;
};

export const getReferencedTextureIDs = (materials: Record<string, MaterialDef>): TextureID[] => {
  const textureIDs: TextureID[] = [];
  for (const mat of Object.values(materials)) {
    if (mat.type === 'basic') {
      continue;
    }

    if (mat.map) {
      textureIDs.push(mat.map);
    }
    if (mat.normalMap) {
      textureIDs.push(mat.normalMap);
    }
    if (mat.roughnessMap) {
      textureIDs.push(mat.roughnessMap);
    }
    if (mat.metalnessMap) {
      textureIDs.push(mat.metalnessMap);
    }
    if (mat.clearcoatNormalMap) {
      textureIDs.push(mat.clearcoatNormalMap);
    }
  }
  return textureIDs;
};
