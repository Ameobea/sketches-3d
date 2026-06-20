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
  // Sample the mesh's own `uv` attribute (e.g. analytic UVs emitted by `rail_sweep`), no
  // reprojection or unwrap. `uvScale` sets tiling frequency via the material's uvTransform.
  | { type: 'mesh_uv'; tileBreaking?: { patchScale: number } }
  | {
      type: 'uv';
      numCones: number;
      flattenToDisk: boolean;
      mapToSphere: boolean;
      enableUVIslandRotation: boolean;
      tileBreaking?: { patchScale: number };
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
import type { CustomShaderOptions } from 'src/viz/shaders/customShader.types';
import { loadTexture } from 'src/viz/textureLoading';
import * as THREE from 'three';
import type { TextureID } from './geotoyAPIClient';
import type { ReverseColorRampParams } from 'src/viz/shaders/reverseColorRamp';

export type PomConfig = NonNullable<CustomShaderOptions['pom']>;

export type TextureFilterMode = 'linear' | 'nearest';

export type PhysicalMaterialTextureField =
  | 'map'
  | 'normalMap'
  | 'roughnessMap'
  | 'metalnessMap'
  | 'clearcoatNormalMap'
  | 'pomHeightMap';

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
  /** Requires `pom` to be set and a non-baseline texturing mode (triplanar or generated UVs). */
  pomHeightMap?: TextureID;
  /** Defaults to 'linear' */
  pomHeightMapFilter?: TextureFilterMode;
  pom?: PomConfig;
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
    /** Shared GLSL emitted before all other slots; declare structs/helpers used by multiple slots. */
    common?: string;
    color?: string;
    /** `vec2 getLightAttenuation(...)` → `(directMul, indirectMul)` for procedural shadow/AO. */
    lightAttenuation?: string;
    roughness?: string;
    metalness?: string;
    iridescence?: string;
    pomHeight?: string;
    /** Analytic POM relief normal; requires `pomHeight`. */
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

export type MaterialDef = BasicMaterialDef | PhysicalMaterialDef;

export interface MaterialDescriptor {
  id: number;
  name: string;
  thumbnailUrl: string | null;
  materialDefinition: MaterialDef;
  ownerId: number;
  ownerName: string;
  createdAt: string;
  isShared: boolean;
}

export const LoadedTextures: Map<TextureID, Promise<THREE.Texture> | THREE.Texture> = new Map();

/* Separate cache: POM heightmaps use RedFormat + mipmaps-off */
const LoadedPomHeightTextures: Map<string, Promise<THREE.Texture> | THREE.Texture> = new Map();

const maybeLoadPomHeightTexture = (
  loader: THREE.ImageBitmapLoader,
  textureId: TextureID | undefined,
  filterPref: TextureFilterMode
): Promise<THREE.Texture> | THREE.Texture | undefined => {
  if (typeof textureId !== 'number') {
    return undefined;
  }
  const key = `${textureId}:${filterPref}`;
  const cached = LoadedPomHeightTextures.get(key);
  if (cached) {
    return cached;
  }
  const mapDef = Textures.textures[textureId];
  if (!mapDef) {
    return undefined;
  }
  const f = filterPref === 'nearest' ? THREE.NearestFilter : THREE.LinearFilter;
  const texP = loadTexture(loader, mapDef.url, {
    format: THREE.RedFormat,
    magFilter: f,
    minFilter: f,
  });
  texP.then(tex => {
    tex.generateMipmaps = false;
    LoadedPomHeightTextures.set(key, tex);
  });
  LoadedPomHeightTextures.set(key, texP);
  return texP;
};

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

interface LoadedTextureBag {
  map: THREE.Texture | undefined;
  normalMap: THREE.Texture | undefined;
  roughnessMap: THREE.Texture | undefined;
  clearcoatNormalMap: THREE.Texture | undefined;
  pomHeightMap: THREE.Texture | undefined;
}

const buildPhysicalShader = (
  def: Extract<MaterialDef, { type: 'physical' }>,
  id: MaterialID,
  textures: LoadedTextureBag
) => {
  const { map, normalMap, roughnessMap, clearcoatNormalMap, pomHeightMap } = textures;
  if (map) {
    map.colorSpace = THREE.SRGBColorSpace;
  }

  const defaultShaders = buildDefaultShaders();
  const customShaders: Partial<CustomShaderShaders> = {};

  if (def.shaders) {
    if (def.shaders.common && def.shaders.common !== defaultShaders.common) {
      customShaders.commonShader = def.shaders.common;
    }
    if (def.shaders.color && def.shaders.color !== defaultShaders.color) {
      customShaders.colorShader = def.shaders.color;
    }
    if (def.shaders.lightAttenuation && def.shaders.lightAttenuation !== defaultShaders.lightAttenuation) {
      customShaders.lightAttenuationShader = def.shaders.lightAttenuation;
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
    if (def.pom && def.shaders.pomHeight && def.shaders.pomHeight !== defaultShaders.pomHeight) {
      customShaders.pomHeightShader = def.shaders.pomHeight;
    }
    if (def.pom && def.shaders.pomNormal && def.shaders.pomNormal !== defaultShaders.pomNormal) {
      customShaders.pomNormalShader = def.shaders.pomNormal;
    }
  }

  const pomActive = !!def.pom && (!!pomHeightMap || !!customShaders.pomHeightShader);

  return buildCustomShader(
    {
      name: id,
      color: new THREE.Color(def.color.r, def.color.g, def.color.b),
      roughness: def.roughness,
      metalness: def.metalness,
      clearcoat: def.clearcoat,
      clearcoatRoughness: def.clearcoatRoughness,
      clearcoatNormalMap,
      clearcoatNormalScale: def.clearcoatNormalScale,
      iridescence: def.iridescence,
      sheen: def.sheen ?? 0,
      sheenColor: def.sheenColor
        ? new THREE.Color(def.sheenColor.r, def.sheenColor.g, def.sheenColor.b)
        : new THREE.Color(0x000000),
      sheenRoughness: def.sheenRoughness ?? 1,
      normalScale: def.normalScale,
      envMapIntensity: def.envMapIntensity,
      uvTransform: new THREE.Matrix3().scale(def.uvScale.x, def.uvScale.y),
      map,
      normalMap,
      roughnessMap,
      pomHeightMap,
      fogMultiplier: def.fogMultiplier,
      mapDisableDistance: def.mapDisableDistance,
      mapDisableTransitionThreshold: def.mapDisableTransitionThreshold,
      ambientLightScale: def.ambientLightScale,
      ambientDistanceAmp: def.ambientDistanceAmp,
    },
    customShaders,
    {
      useTriplanarMapping: !def.textureMapping || def.textureMapping?.type === 'triplanar',
      tileBreaking:
        def.textureMapping && def.textureMapping.type !== 'triplanar' && def.textureMapping.tileBreaking
          ? { type: 'neyret', patchScale: def.textureMapping.tileBreaking.patchScale }
          : undefined,
      useGeneratedUVs: false,
      pom: pomActive ? def.pom : undefined,
    }
  );
};

export const buildMaterial = (
  loader: THREE.ImageBitmapLoader,
  def: MaterialDef,
  id: MaterialID
): Promise<THREE.Material> | THREE.Material => {
  if (def.type === 'basic') {
    return new THREE.MeshBasicMaterial({
      name: id,
      color: new THREE.Color(def.color.r, def.color.g, def.color.b),
    });
  } else if (def.type === 'physical') {
    const mapP = maybeLoadTexture(loader, def.map);
    const normalMapP = maybeLoadTexture(loader, def.normalMap);
    const roughnessMapP = maybeLoadTexture(loader, def.roughnessMap);
    const clearcoatNormalMapP = maybeLoadTexture(loader, def.clearcoatNormalMap);
    const pomHeightMapP = maybeLoadPomHeightTexture(
      loader,
      def.pomHeightMap,
      def.pomHeightMapFilter ?? 'linear'
    );

    const slotsP = [mapP, normalMapP, roughnessMapP, clearcoatNormalMapP, pomHeightMapP] as const;
    if (slotsP.every(v => !(v instanceof Promise))) {
      return buildPhysicalShader(def, id, {
        map: mapP as THREE.Texture | undefined,
        normalMap: normalMapP as THREE.Texture | undefined,
        roughnessMap: roughnessMapP as THREE.Texture | undefined,
        clearcoatNormalMap: clearcoatNormalMapP as THREE.Texture | undefined,
        pomHeightMap: pomHeightMapP as THREE.Texture | undefined,
      });
    }
    return Promise.all(slotsP).then(([map, normalMap, roughnessMap, clearcoatNormalMap, pomHeightMap]) =>
      buildPhysicalShader(def, id, { map, normalMap, roughnessMap, clearcoatNormalMap, pomHeightMap })
    );
  } else {
    def satisfies never;
    throw new Error(`Unknown material type: ${(def as any).type}`);
  }
};

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
    pomHeight: undefined,
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
