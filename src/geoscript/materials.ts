export type MaterialID = string;

/**
 * webgl-style; each component should be [0, 1]
 */
export interface RGBColor {
  r: number;
  g: number;
  b: number;
}

export type TextureMapping =
  | { type: 'triplanar' }
  | {
      type: 'uv';
      numCones: number;
      flattenToDisk: boolean;
      mapToSphere: boolean;
      enableUVIslandRotation: boolean;
    };

export interface BasicMaterialDef {
  type: 'basic';
  name: string;
  color: RGBColor;
  shaders?: {
    color?: string;
  };
  textureMapping?: TextureMapping;
}

import { Textures } from 'src/viz/scenes/geoscriptPlayground/materialEditor/state.svelte';
import { buildCustomShader, type CustomShaderShaders } from 'src/viz/shaders/customShader';
import { loadTexture } from 'src/viz/textureLoading';
import * as THREE from 'three';
import type { TextureID } from './geotoyAPIClient';
import type { ReverseColorRampParams } from 'src/viz/shaders/reverseColorRamp';

export interface PhysicalMaterialDef {
  type: 'physical';
  name: string;
  color: RGBColor;
  roughness: number;
  metalness: number;
  clearcoat: number;
  clearcoatRoughness: number;
  iridescence: number;
  sheen?: number;
  sheenColor?: RGBColor;
  sheenRoughness?: number;
  normalScale: number;
  uvScale: { x: number; y: number };
  map?: TextureID;
  normalMap?: TextureID;
  roughnessMap?: TextureID;
  metalnessMap?: TextureID;
  fogMultiplier?: number;
  mapDisableDistance?: number | null;
  mapDisableTransitionThreshold?: number;
  ambientLightScale?: number;
  ambientDistanceAmp?: {
    falloffStartDistance: number;
    falloffEndDistance: number;
    exponent?: number;
    ampFactor: number;
  };
  shaders?: {
    color?: string;
    roughness?: string;
    metalness?: string;
    iridescence?: string;
  };
  reverseColorRamps?: {
    roughness?: ReverseColorRampParams;
    metalness?: ReverseColorRampParams;
    clearcoat?: ReverseColorRampParams;
    clearcoatRoughness?: ReverseColorRampParams;
    iridescence?: ReverseColorRampParams;
    sheen?: ReverseColorRampParams;
  };
  textureMapping?: TextureMapping;
}

export type MaterialDef = BasicMaterialDef | PhysicalMaterialDef;

const LoadedTextures: Map<TextureID, Promise<THREE.Texture> | THREE.Texture> = new Map();

const maybeLoadTexture = (
  loader: THREE.ImageBitmapLoader,
  textureId?: TextureID
): Promise<THREE.Texture> | THREE.Texture | undefined => {
  if (typeof textureId !== 'number') {
    return undefined;
  }

  const cached = LoadedTextures.get(textureId);
  if (cached) {
    return cached;
  }

  const mapDef = Textures.textures[textureId];
  if (!mapDef) {
    // console.warn(`Tried to load undefined texture: ${textureId}`);
    return undefined;
  }

  const texP = loadTexture(loader, mapDef.url);
  texP.then(tex => {
    LoadedTextures.set(textureId, tex);
  });
  LoadedTextures.set(textureId, texP);
  return texP;
};

const buildPhysicalShader = (
  def: Extract<MaterialDef, { type: 'physical' }>,
  id: MaterialID,
  map: THREE.Texture | undefined,
  normalMap: THREE.Texture | undefined,
  roughnessMap: THREE.Texture | undefined,
  useTriplanarMapping: boolean
) => {
  const defaultShaders = buildDefaultShaders();
  const customShaders: Partial<CustomShaderShaders> = {};

  if (def.shaders) {
    if (def.shaders.color && def.shaders.color !== defaultShaders.color) {
      customShaders.colorShader = def.shaders.color;
    }
    if (def.reverseColorRamps?.roughness) {
      customShaders.roughnessReverseColorRamp = def.reverseColorRamps.roughness;
    } else if (def.shaders.roughness && def.shaders.roughness !== defaultShaders.roughness) {
      customShaders.roughnessShader = def.shaders.roughness;
    }
    if (def.reverseColorRamps?.metalness) {
      customShaders.metalnessReverseColorRamp = def.reverseColorRamps.metalness;
    } else if (def.shaders.metalness && def.shaders.metalness !== defaultShaders.metalness) {
      customShaders.metalnessShader = def.shaders.metalness;
    }
    if (def.reverseColorRamps?.iridescence) {
      customShaders.iridescenceReverseColorRamp = def.reverseColorRamps.iridescence;
    } else if (def.shaders.iridescence && def.shaders.iridescence !== defaultShaders.iridescence) {
      customShaders.iridescenceShader = def.shaders.iridescence;
    }
  }

  return buildCustomShader(
    {
      name: id,
      color: new THREE.Color(def.color.r, def.color.g, def.color.b),
      roughness: def.roughness,
      metalness: def.metalness,
      clearcoat: def.clearcoat,
      clearcoatRoughness: def.clearcoatRoughness,
      iridescence: def.iridescence,
      sheen: def.sheen ?? 0,
      sheenColor: def.sheenColor
        ? new THREE.Color(def.sheenColor.r, def.sheenColor.g, def.sheenColor.b)
        : new THREE.Color(0x000000),
      sheenRoughness: def.sheenRoughness ?? 1,
      normalScale: def.normalScale,
      uvTransform: new THREE.Matrix3().scale(def.uvScale.x, def.uvScale.y),
      map,
      normalMap,
      roughnessMap,
      fogMultiplier: def.fogMultiplier,
      mapDisableDistance: def.mapDisableDistance,
      mapDisableTransitionThreshold: def.mapDisableTransitionThreshold,
      ambientLightScale: def.ambientLightScale,
      ambientDistanceAmp: def.ambientDistanceAmp,
    },
    customShaders,
    { useTriplanarMapping, tileBreaking: undefined, useGeneratedUVs: false }
  );
};

export const buildMaterial = (
  loader: THREE.ImageBitmapLoader,
  def: MaterialDef,
  id: MaterialID
): Promise<THREE.Material> | THREE.Material => {
  const textureMappingType = def.textureMapping?.type;
  const useTriplanarMapping = textureMappingType === 'triplanar' || textureMappingType === undefined;
  if (def.type === 'basic') {
    return new THREE.MeshBasicMaterial({
      name: id,
      color: new THREE.Color(def.color.r, def.color.g, def.color.b),
    });
  } else if (def.type === 'physical') {
    const mapP: Promise<THREE.Texture> | THREE.Texture | undefined = maybeLoadTexture(loader, def.map);
    const normalMapP: Promise<THREE.Texture> | THREE.Texture | undefined = maybeLoadTexture(
      loader,
      def.normalMap
    );
    const roughnessMapP: Promise<THREE.Texture> | THREE.Texture | undefined = maybeLoadTexture(
      loader,
      def.roughnessMap
    );

    if (
      !(mapP instanceof Promise) &&
      !(normalMapP instanceof Promise) &&
      !(roughnessMapP instanceof Promise)
    ) {
      return buildPhysicalShader(def, id, mapP, normalMapP, roughnessMapP, useTriplanarMapping);
    }

    return Promise.all([mapP, normalMapP, roughnessMapP] as const).then(([map, normalMap, roughnessMap]) =>
      buildPhysicalShader(def, id, map, normalMap, roughnessMap, useTriplanarMapping)
    );
  } else {
    def satisfies never;
    throw new Error(`Unknown material type: ${(def as any).type}`);
  }
};

export const buildDefaultShaders = (): NonNullable<PhysicalMaterialDef['shaders']> => ({
  color: `vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  return vec4(baseColor, 1.0);
}`,
  roughness: `float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  return baseRoughness;
}`,
  metalness: `float getCustomMetalness(vec3 pos, vec3 normal, float baseMetalness, float curTimeSeconds, SceneCtx ctx) {
  return baseMetalness;
}`,
  iridescence: `float getCustomIridescence(vec3 pos, vec3 normal, float baseIridescence, float curTimeSeconds, SceneCtx ctx) {
  return baseIridescence;
}`,
});

export interface MaterialDefinitions {
  materials: Record<MaterialID, MaterialDef>;
  defaultMaterialID: MaterialID | null;
}

export const buildDefaultMaterial = (name: string): MaterialDef => ({
  type: 'physical',
  name,
  color: { r: 0.8, g: 0.8, b: 0.8 },
  roughness: 0.95,
  metalness: 0.1,
  clearcoat: 0,
  clearcoatRoughness: 0,
  iridescence: 0,
  sheen: 0,
  sheenColor: new THREE.Color(0x000000),
  sheenRoughness: 1,
  normalScale: 1,
  uvScale: { x: 0.13, y: 0.13 },
  mapDisableDistance: null,
  mapDisableTransitionThreshold: undefined,
  fogMultiplier: 1,
  ambientDistanceAmp: undefined,
  map: 1,
  normalMap: 2,
  roughnessMap: undefined,
  ambientLightScale: 1,
  shaders: {
    color: undefined,
    roughness: undefined,
    metalness: undefined,
    iridescence: undefined,
  },
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
