import * as THREE from 'three';

import { buildMaterial as buildSharedMaterial } from 'src/viz/materials';
import {
  TEXTURE_SLOT_META,
  TEXTURE_SLOTS,
  type CustomShaderMatDef,
  type CustomBasicShaderMatDef,
} from 'src/viz/materials/schema';
import { loadTexture } from 'src/viz/textureLoading';
import { Textures } from 'src/geotoy/panels/materialEditor/state.svelte';
import {
  getProceduralTexture,
  isProceduralHandle,
  isStackHandle,
} from 'src/geotoy/modules/proceduralTextures';
import type { TextureID } from './geotoyAPIClient';

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
  if (isProceduralHandle(handle)) {
    return getProceduralTexture(handle);
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
  if (isProceduralHandle(handle)) {
    return getProceduralTexture(handle);
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
  const slotPs = TEXTURE_SLOTS.map(slot => {
    const h = p[slot];
    const meta = TEXTURE_SLOT_META[slot];
    if (h != null && !('stacks' in meta) && isStackHandle(h)) {
      throw new Error(`Texture stacks are not supported for the ${slot} slot`);
    }
    return 'red' in meta ? maybeLoadPomHeightTexture(loader, h) : maybeLoadTexture(loader, h);
  });

  const uniformTexUrls = Object.values(def.shaders?.customUniforms ?? {}).flatMap(u =>
    u.type === 'sampler2D' && /^https?:\/\//.test(u.value) ? [u.value] : []
  );
  const uniformTexPs = uniformTexUrls.map(url => loadUrlTexture(loader, url));

  const finish = (slotTexs: (THREE.Texture | undefined)[], uniformTexs: THREE.Texture[]): THREE.Material => {
    const textures = new Map<string, THREE.Texture>();
    TEXTURE_SLOTS.forEach((slot, i) => {
      const handle = p[slot];
      const tex = slotTexs[i];
      if (handle == null || !tex) return;
      // Procedural textures hold linear geoscript values, never sRGB-encoded bytes.
      if ('srgb' in TEXTURE_SLOT_META[slot] && !isProceduralHandle(handle)) {
        tex.colorSpace = THREE.SRGBColorSpace;
      }
      textures.set(handle, tex);
    });
    uniformTexUrls.forEach((url, i) => textures.set(url, uniformTexs[i]));
    const mat = buildSharedMaterial(def, textures);
    mat.name = id;
    return mat;
  };

  if ([...slotPs, ...uniformTexPs].every(v => !(v instanceof Promise))) {
    return finish(slotPs as (THREE.Texture | undefined)[], uniformTexPs as THREE.Texture[]);
  }
  return Promise.all([Promise.all(slotPs), Promise.all(uniformTexPs)]).then(([slotTexs, uniformTexs]) =>
    finish(slotTexs, uniformTexs)
  );
};

const linearRgbToSrgbHex = (c: { r: number; g: number; b: number }): number =>
  new THREE.Color().setRGB(c.r, c.g, c.b).getHex(THREE.SRGBColorSpace);

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
