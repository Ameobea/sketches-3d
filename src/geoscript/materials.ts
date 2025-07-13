export type MaterialID = string;

/**
 * webgl-style; each component should be [0, 1]
 */
export interface RGBColor {
  r: number;
  g: number;
  b: number;
}

export interface BasicMaterialDef {
  type: 'basic';
  name: string;
  color: RGBColor;
}

import { Textures } from 'src/viz/scenes/geoscriptPlayground/materialEditor/state.svelte';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadTexture } from 'src/viz/textureLoading';
import * as THREE from 'three';
import type { TextureID } from './geotoyAPIClient';

export interface PhysicalMaterialDef {
  type: 'physical';
  name: string;
  color: RGBColor;
  roughness: number;
  metalness: number;
  clearcoat: number;
  clearcoatRoughness: number;
  iridescence: number;
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
}

export type MaterialDef = BasicMaterialDef | PhysicalMaterialDef;

const LoadedTextures: Map<TextureID, Promise<THREE.Texture>> = new Map();

const maybeLoadTexture = async (
  loader: THREE.ImageBitmapLoader,
  textureId?: TextureID
): Promise<THREE.Texture | undefined> => {
  if (typeof textureId !== 'number') {
    return undefined;
  }

  const cached = LoadedTextures.get(textureId);
  if (cached) {
    return cached;
  }

  const mapDef = Textures.textures[textureId];
  if (!mapDef) {
    console.warn(`Tried to load undefined texture: ${textureId}`);
    return undefined;
  }

  const texP = loadTexture(loader, mapDef.url);
  LoadedTextures.set(textureId, texP);
  return texP;
};

export const buildMaterial = async (
  loader: THREE.ImageBitmapLoader,
  def: MaterialDef,
  id: MaterialID
): Promise<THREE.Material> => {
  if (def.type === 'basic') {
    return new THREE.MeshBasicMaterial({
      name: id,
      color: new THREE.Color(def.color.r, def.color.g, def.color.b),
    });
  } else if (def.type === 'physical') {
    const mapP: Promise<THREE.Texture | undefined> = maybeLoadTexture(loader, def.map);
    const normalMapP: Promise<THREE.Texture | undefined> = maybeLoadTexture(loader, def.normalMap);
    const roughnessMapP: Promise<THREE.Texture | undefined> = maybeLoadTexture(loader, def.roughnessMap);

    const [map, normalMap, roughnessMap] = await Promise.all([mapP, normalMapP, roughnessMapP] as const);

    return buildCustomShader(
      {
        name: id,
        color: new THREE.Color(def.color.r, def.color.g, def.color.b),
        roughness: def.roughness,
        metalness: def.metalness,
        clearcoat: def.clearcoat,
        clearcoatRoughness: def.clearcoatRoughness,
        iridescence: def.iridescence,
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
      {}, // TODO: eventually want to support custom shaders
      { useTriplanarMapping: true, tileBreaking: undefined, useGeneratedUVs: false }
    );
  } else {
    def satisfies never;
    throw new Error(`Unknown material type: ${(def as any).type}`);
  }
};

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
  normalScale: 1,
  uvScale: { x: 0.13, y: 0.13 },
  mapDisableDistance: null,
  mapDisableTransitionThreshold: undefined,
  fogMultiplier: 1,
  ambientDistanceAmp: undefined,
  map: 1,
  normalMap: 2,
  roughnessMap: undefined,
});

export const buildDefaultMaterialDefinitions = (): MaterialDefinitions => ({
  materials: {
    gray_fossil_rock: buildDefaultMaterial('gray_fossil_rock'),
  },
  defaultMaterialID: 'gray_fossil_rock',
});
