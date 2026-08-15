import type * as THREE from 'three';
import { DataTexture } from 'three';
import { getMultipleTextures, type TextureID } from 'src/geoscript/geotoyAPIClient';
import { LoadedTextures, type MaterialDef } from 'src/geoscript/materials';
import { Textures } from 'src/geotoy/panels/materialEditor/state.svelte';
import { loadTexture } from 'src/viz/textureLoading';

let fallbackTex: THREE.Texture | null = null;
const getFallbackTexture = (): THREE.Texture => {
  if (!fallbackTex) {
    fallbackTex = new DataTexture(new Uint8Array([255, 0, 255, 255]), 1, 1);
    fallbackTex.needsUpdate = true;
  }
  return fallbackTex;
};

export const fetchAndSetTextures = async (loader: THREE.ImageBitmapLoader, textureIDs: TextureID[]) => {
  const missingTextureIDs = textureIDs.filter(id => !LoadedTextures.has(id));
  if (missingTextureIDs.length === 0) {
    return;
  }

  const resolvers: Map<TextureID, (tex: THREE.Texture) => void> = new Map();
  for (const id of missingTextureIDs) {
    const p = new Promise<THREE.Texture>(resolve => {
      resolvers.set(id, resolve);
    });
    LoadedTextures.set(id, p);
  }

  const adminToken = new URLSearchParams(window.location.search).get('admin_token') ?? undefined;
  try {
    const textures = await getMultipleTextures(missingTextureIDs, undefined, adminToken);
    const allTextures = { ...Textures.textures };
    for (const tex of textures) {
      allTextures[tex.id] = tex;
      const resolver = resolvers.get(tex.id);
      if (resolver) {
        resolvers.delete(tex.id);
        const threeTexP = loadTexture(loader, tex.url);
        LoadedTextures.set(tex.id, threeTexP);
        threeTexP.then(threeTex => {
          resolver(threeTex);
          LoadedTextures.set(tex.id, threeTex);
        });
      }
    }
    Textures.textures = { ...Textures.textures, ...allTextures };
  } finally {
    // Ids the API didn't return (deleted textures, failed fetch) would otherwise strand
    // forever-pending placeholders — meshes stuck on HiddenMat and headless runs hanging
    // to timeout. Settle the placeholder for builds that already captured it, drop it
    // from the cache (enables retry), and surface the failure.
    for (const [id, resolve] of resolvers) {
      resolve(getFallbackTexture());
      LoadedTextures.delete(id);
      console.error(`Texture ${id} could not be fetched; referencing materials will lack it`);
    }
  }
};

export const referencedTextureIDsForDef = (mat: MaterialDef): TextureID[] => {
  if (mat.type !== 'customShader' || !mat.props) {
    return [];
  }
  const p = mat.props;
  const textureIDs: TextureID[] = [];
  for (const handle of [
    p.map,
    p.normalMap,
    p.roughnessMap,
    p.metalnessMap,
    p.clearcoatNormalMap,
    p.pomHeightMap,
  ]) {
    // Non-numeric handles (procedural refs) aren't library textures and have no metadata.
    if (handle != null && Number.isFinite(Number(handle))) {
      textureIDs.push(Number(handle));
    }
  }
  return textureIDs;
};

export const getReferencedTextureIDs = (materials: Record<string, MaterialDef>): TextureID[] =>
  Object.values(materials).flatMap(referencedTextureIDsForDef);
