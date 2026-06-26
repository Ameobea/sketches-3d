import * as THREE from 'three';

import type {
  CustomShaderMatDef,
  CustomBasicShaderMatDef,
  ShaderShadersJson,
} from 'src/viz/materials/schema';
import type { CustomShaderOptions } from 'src/viz/shaders/customShader.types';
import type { ReverseColorRampParams } from 'src/viz/shaders/reverseColorRamp';
import type { TextureID } from './geotoyAPIClient';

// ---------------------------------------------------------------------------
// Legacy Geotoy `physical`/`basic` material shape — the input the one-shot DB
// migration reads. Nothing at runtime authors these anymore (the editor + build
// path use the shared `customShader` shape); kept here purely as migration input.
// ---------------------------------------------------------------------------

export interface RGBColor {
  r: number;
  g: number;
  b: number;
}

export type TextureMapping =
  | { type: 'triplanar' }
  | { type: 'mesh_uv'; tileBreaking?: { patchScale: number } }
  | {
      type: 'uv';
      numCones: number;
      flattenToDisk: boolean;
      mapToSphere: boolean;
      enableUVIslandRotation: boolean;
      tileBreaking?: { patchScale: number };
    };

type PomConfig = NonNullable<CustomShaderOptions['pom']>;

export interface PhysicalMaterialDef {
  type: 'physical';
  name: string;
  color: RGBColor;
  roughness: number;
  metalness: number;
  clearcoat: number;
  clearcoatRoughness: number;
  clearcoatNormalMap?: TextureID;
  clearcoatNormalScale?: number;
  iridescence: number;
  sheen?: number;
  sheenColor?: RGBColor;
  sheenRoughness?: number;
  normalScale: number;
  envMapIntensity?: number;
  uvScale: { x: number; y: number };
  map?: TextureID;
  normalMap?: TextureID;
  roughnessMap?: TextureID;
  metalnessMap?: TextureID;
  pomHeightMap?: TextureID;
  pomHeightMapFilter?: 'linear' | 'nearest';
  pom?: PomConfig;
  useOrenNayarDiffuse?: boolean;
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
    common?: string;
    color?: string;
    lightAttenuation?: string;
    roughness?: string;
    metalness?: string;
    iridescence?: string;
    pomHeight?: string;
    pomNormal?: string;
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

export interface BasicMaterialDef {
  type: 'basic';
  name: string;
  color: RGBColor;
  shaders?: { color?: string };
  textureMapping?: TextureMapping;
}

export const linearRgbToSrgbHex = (c: { r: number; g: number; b: number }): number =>
  new THREE.Color().setRGB(c.r, c.g, c.b).getHex(THREE.SRGBColorSpace);

export const idToHandle = (id: TextureID | undefined): string | undefined =>
  id != null ? String(id) : undefined;

export const buildDefaultShaders = (): NonNullable<PhysicalMaterialDef['shaders']> => ({
  common: `// Shared GLSL emitted before every other slot — declare structs, constants, and
// helper functions used by multiple slots here so the logic lives in one place.`,
  color: `vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  return vec4(baseColor, 1.0);
}`,
  lightAttenuation: `// Returns (directMul, indirectMul) in [0,1], scaling direct/indirect light for
// procedural shadow + AO. (1.0, 1.0) = no attenuation.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  return vec2(1.0);
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
  pomHeight: `// Carved depth in [0, 1]: 0 = base surface, 1 = a full pom.depth carved inward.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  return 0.0;
}`,
  pomNormal: `// Closed-form relief normal for the carved POM floor (world space); requires pomHeight.
// \`aa\` is the pixel-footprint half-width in world units — fade detail with reliefAAFade(aa, w).
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  return N;
}`,
});

/**
 * Lower a Geotoy `physical` def to the shared `customShader` def. Geotoy treats `{r,g,b}` as linear
 * working-space color, while the shared format stores an sRGB hex the shader decodes — hence the
 * `linearRgbToSrgbHex` round-trip, which preserves the rendered look. Default shader slots are dropped
 * (absent ⇒ none), and `textureMapping` splits into UV options + an optional `meshUvUnwrap` recipe.
 */
export const geotoyPhysicalToShared = (def: PhysicalMaterialDef): CustomShaderMatDef => {
  const d = buildDefaultShaders();
  const s = def.shaders;
  const shaders: ShaderShadersJson = {};
  if (s) {
    if (s.common && s.common !== d.common) shaders.commonShader = s.common;
    if (s.color && s.color !== d.color) shaders.colorShader = s.color;
    if (s.lightAttenuation && s.lightAttenuation !== d.lightAttenuation)
      shaders.lightAttenuationShader = s.lightAttenuation;
    if (def.reverseColorRamps?.roughness) shaders.roughnessReverseColorRamp = def.reverseColorRamps.roughness;
    else if (s.roughness && s.roughness !== d.roughness) shaders.roughnessShader = s.roughness;
    if (def.reverseColorRamps?.metalness) shaders.metalnessReverseColorRamp = def.reverseColorRamps.metalness;
    else if (s.metalness && s.metalness !== d.metalness) shaders.metalnessShader = s.metalness;
    if (def.reverseColorRamps?.iridescence)
      shaders.iridescenceReverseColorRamp = def.reverseColorRamps.iridescence;
    else if (s.iridescence && s.iridescence !== d.iridescence) shaders.iridescenceShader = s.iridescence;
    if (def.pom && s.pomHeight && s.pomHeight !== d.pomHeight) shaders.pomHeightShader = s.pomHeight;
    if (def.pom && s.pomNormal && s.pomNormal !== d.pomNormal) shaders.pomNormalShader = s.pomNormal;
  }

  const tm = def.textureMapping;
  const triplanar = !tm || tm.type === 'triplanar';
  const tileBreaking =
    tm && tm.type !== 'triplanar' && tm.tileBreaking
      ? ({ type: 'neyret', patchScale: tm.tileBreaking.patchScale } as const)
      : undefined;
  const pomActive = !!def.pom && (def.pomHeightMap != null || !!shaders.pomHeightShader);

  return {
    type: 'customShader',
    props: {
      color: linearRgbToSrgbHex(def.color),
      roughness: def.roughness,
      metalness: def.metalness,
      clearcoat: def.clearcoat,
      clearcoatRoughness: def.clearcoatRoughness,
      clearcoatNormalScale: def.clearcoatNormalScale,
      iridescence: def.iridescence,
      sheen: def.sheen ?? 0,
      sheenColor: def.sheenColor ? linearRgbToSrgbHex(def.sheenColor) : 0x000000,
      sheenRoughness: def.sheenRoughness ?? 1,
      normalScale: def.normalScale,
      envMapIntensity: def.envMapIntensity,
      uvScale: [def.uvScale.x, def.uvScale.y],
      map: idToHandle(def.map),
      normalMap: idToHandle(def.normalMap),
      roughnessMap: idToHandle(def.roughnessMap),
      metalnessMap: idToHandle(def.metalnessMap),
      clearcoatNormalMap: idToHandle(def.clearcoatNormalMap),
      pomHeightMap: idToHandle(def.pomHeightMap),
      fogMultiplier: def.fogMultiplier,
      mapDisableDistance: def.mapDisableDistance,
      mapDisableTransitionThreshold: def.mapDisableTransitionThreshold,
      ambientLightScale: def.ambientLightScale,
      ambientDistanceAmp: def.ambientDistanceAmp,
    },
    shaders: Object.keys(shaders).length ? shaders : undefined,
    options: {
      useTriplanarMapping: triplanar,
      useGeneratedUVs: false,
      tileBreaking,
      pom: pomActive ? def.pom : undefined,
      useOrenNayarDiffuse: def.useOrenNayarDiffuse,
    },
    meshUvUnwrap:
      tm && tm.type === 'uv'
        ? {
            numCones: tm.numCones,
            flattenToDisk: tm.flattenToDisk,
            mapToSphere: tm.mapToSphere,
            // Legacy `uv` defs predate this field; the live persistence backfill defaults it to true.
            enableUVIslandRotation: tm.enableUVIslandRotation ?? true,
          }
        : undefined,
  };
};

/** Geotoy `basic` (unlit, color-only `MeshBasicMaterial`) → shared `customBasicShader`. */
export const geotoyBasicToShared = (def: BasicMaterialDef): CustomBasicShaderMatDef => ({
  type: 'customBasicShader',
  props: { color: linearRgbToSrgbHex(def.color) },
});
