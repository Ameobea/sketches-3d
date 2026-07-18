import * as THREE from 'three';

import { buildMaterial as buildSharedMaterial } from 'src/viz/materials';
import type { CustomShaderMatDef, CustomBasicShaderMatDef } from 'src/viz/materials/schema';
import { loadTexture } from 'src/viz/textureLoading';
import { Textures } from 'src/viz/scenes/geoscriptPlayground/materialEditor/state.svelte';
import { buildDefaultShaders, linearRgbToSrgbHex, type RGBColor } from './geotoyMaterialConvert';
import type { TextureID } from './geotoyAPIClient';

export { buildDefaultShaders };
export type { RGBColor };
export type { CustomShaderMatDef, CustomBasicShaderMatDef } from 'src/viz/materials/schema';

/** Geotoy materials are always shader-based (no level-only `generated` variant) and always carry a
 *  `name` — geoscript nodes reference materials by name and the editor keys the palette on uuids. */
export type MaterialDef =
  | (CustomShaderMatDef & { name: string })
  | (CustomBasicShaderMatDef & { name: string });

export type MaterialID = string;

export interface MaterialDefinitions {
  materials: Record<MaterialID, MaterialDef>;
  defaultMaterialID: MaterialID | null;
}

export interface MaterialDescriptor {
  id: number;
  name: string;
  description: string;
  thumbnailUrl: string | null;
  materialDefinition: MaterialDef;
  ownerId: number;
  ownerName: string;
  createdAt: string;
  isShared: boolean;
  tags: string[];
}

export type { PhysicalMaterialTextureField } from 'src/viz/materials/ui/host';

export const LoadedTextures: Map<TextureID, Promise<THREE.Texture> | THREE.Texture> = new Map();

/* Separate cache: POM heightmaps use RedFormat + mipmaps-off */
const LoadedPomHeightTextures: Map<string, Promise<THREE.Texture> | THREE.Texture> = new Map();

const maybeLoadTexture = (
  loader: THREE.ImageBitmapLoader,
  handle?: string
): Promise<THREE.Texture> | THREE.Texture | undefined => {
  if (handle == null) {
    return undefined;
  }
  const id = Number(handle);
  const cached = LoadedTextures.get(id);
  if (cached) {
    return cached;
  }
  const mapDef = Textures.textures[handle];
  if (!mapDef) {
    return undefined;
  }
  const texP = loadTexture(loader, mapDef.url);
  texP.then(tex => LoadedTextures.set(id, tex));
  LoadedTextures.set(id, texP);
  return texP;
};

const maybeLoadPomHeightTexture = (
  loader: THREE.ImageBitmapLoader,
  handle?: string
): Promise<THREE.Texture> | THREE.Texture | undefined => {
  if (handle == null) {
    return undefined;
  }
  const cached = LoadedPomHeightTextures.get(handle);
  if (cached) {
    return cached;
  }
  const mapDef = Textures.textures[handle];
  if (!mapDef) {
    return undefined;
  }
  const texP = loadTexture(loader, mapDef.url, {
    format: THREE.RedFormat,
    magFilter: THREE.LinearFilter,
    minFilter: THREE.LinearFilter,
  });
  texP.then(tex => {
    tex.generateMipmaps = false;
    LoadedPomHeightTextures.set(handle, tex);
  });
  LoadedPomHeightTextures.set(handle, texP);
  return texP;
};

const EMPTY_TEXTURES: ReadonlyMap<string, THREE.Texture> = new Map();

/* sampler2D custom uniforms in geotoy-format defs hold direct URLs (no textures registry);
 * loaded with repeat wrap + trilinear mips and registered under the URL itself as the key. */
const LoadedUrlTextures: Map<string, Promise<THREE.Texture> | THREE.Texture> = new Map();

const loadUrlTexture = (
  loader: THREE.ImageBitmapLoader,
  url: string
): Promise<THREE.Texture> | THREE.Texture => {
  const cached = LoadedUrlTextures.get(url);
  if (cached) {
    return cached;
  }
  const texP = loadTexture(loader, url, {
    magFilter: THREE.LinearFilter,
    minFilter: THREE.LinearMipmapLinearFilter,
  });
  texP.then(tex => LoadedUrlTextures.set(url, tex));
  LoadedUrlTextures.set(url, texP);
  return texP;
};

export const buildMaterial = (
  loader: THREE.ImageBitmapLoader,
  def: MaterialDef,
  id: MaterialID
): Promise<THREE.Material> | THREE.Material => {
  if (def.type === 'customBasicShader') {
    const mat = buildSharedMaterial(def, EMPTY_TEXTURES);
    mat.name = id;
    return mat;
  }
  if (def.type !== 'customShader') {
    throw new Error(`Unsupported material type: ${(def as { type: string }).type}`);
  }

  const p = def.props ?? {};
  const mapP = maybeLoadTexture(loader, p.map);
  const normalMapP = maybeLoadTexture(loader, p.normalMap);
  const roughnessMapP = maybeLoadTexture(loader, p.roughnessMap);
  const metalnessMapP = maybeLoadTexture(loader, p.metalnessMap);
  const clearcoatNormalMapP = maybeLoadTexture(loader, p.clearcoatNormalMap);
  const pomHeightMapP = maybeLoadPomHeightTexture(loader, p.pomHeightMap);

  const uniformTexUrls = Object.values(def.shaders?.customUniforms ?? {}).flatMap(u =>
    u.type === 'sampler2D' && /^https?:\/\//.test(u.value) ? [u.value] : []
  );
  const uniformTexPs = uniformTexUrls.map(url => loadUrlTexture(loader, url));

  type Tex = THREE.Texture | undefined;
  const finish = (
    r: {
      map: Tex;
      normalMap: Tex;
      roughnessMap: Tex;
      metalnessMap: Tex;
      clearcoatNormalMap: Tex;
      pomHeightMap: Tex;
    },
    uniformTexs: THREE.Texture[]
  ): THREE.Material => {
    const textures = new Map<string, THREE.Texture>();
    const put = (handle: string | undefined, tex: Tex, srgb = false) => {
      if (handle != null && tex) {
        if (srgb) tex.colorSpace = THREE.SRGBColorSpace;
        textures.set(handle, tex);
      }
    };
    put(p.map, r.map, true);
    put(p.normalMap, r.normalMap);
    put(p.roughnessMap, r.roughnessMap);
    put(p.metalnessMap, r.metalnessMap);
    put(p.clearcoatNormalMap, r.clearcoatNormalMap);
    put(p.pomHeightMap, r.pomHeightMap);
    uniformTexUrls.forEach((url, i) => textures.set(url, uniformTexs[i]));
    const mat = buildSharedMaterial(def, textures);
    mat.name = id;
    return mat;
  };

  const slotsP = [
    mapP,
    normalMapP,
    roughnessMapP,
    metalnessMapP,
    clearcoatNormalMapP,
    pomHeightMapP,
  ] as const;
  if ([...slotsP, ...uniformTexPs].every(v => !(v instanceof Promise))) {
    return finish(
      {
        map: mapP as Tex,
        normalMap: normalMapP as Tex,
        roughnessMap: roughnessMapP as Tex,
        metalnessMap: metalnessMapP as Tex,
        clearcoatNormalMap: clearcoatNormalMapP as Tex,
        pomHeightMap: pomHeightMapP as Tex,
      },
      uniformTexPs as THREE.Texture[]
    );
  }
  return Promise.all([Promise.all(slotsP), Promise.all(uniformTexPs)]).then(
    ([[map, normalMap, roughnessMap, metalnessMap, clearcoatNormalMap, pomHeightMap], uniformTexs]) =>
      finish({ map, normalMap, roughnessMap, metalnessMap, clearcoatNormalMap, pomHeightMap }, uniformTexs)
  );
};

export const buildDefaultMaterial = (name: string): MaterialDef => ({
  type: 'customShader',
  name,
  props: {
    color: linearRgbToSrgbHex({ r: 0.8, g: 0.8, b: 0.8 }),
    roughness: 0.95,
    metalness: 0.1,
    clearcoat: 0,
    clearcoatRoughness: 0,
    iridescence: 0,
    sheen: 0,
    sheenColor: 0x000000,
    sheenRoughness: 1,
    normalScale: 1,
    uvScale: [0.13, 0.13],
    map: '1',
    normalMap: '2',
    fogMultiplier: 1,
    mapDisableDistance: null,
    ambientLightScale: 1,
  },
  options: { useTriplanarMapping: true, useGeneratedUVs: false },
});

export const buildDefaultMaterialDefinitions = (): MaterialDefinitions => ({
  materials: {
    default: buildDefaultMaterial('default'),
  },
  defaultMaterialID: 'default',
});

export const LineMat = new THREE.LineBasicMaterial({
  color: 0x00ff00,
  linewidth: 2,
});
export const HiddenMat = new THREE.MeshBasicMaterial({ transparent: true, opacity: 0 });
export const FallbackMat = new THREE.MeshBasicMaterial({
  color: 0x888888,
});
export const WireframeMat = new THREE.MeshBasicMaterial({
  color: 0xdf00df,
  wireframe: true,
});
export const NormalMat = new THREE.MeshNormalMaterial();
