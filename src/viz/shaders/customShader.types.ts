import type * as THREE from 'three';

import type { ReverseColorRampParams } from './reverseColorRamp';
import type { TriplanarMappingParams } from './triplanarMapping';

export type { PlayerShadowParams } from './customShader';

export interface AmbientDistanceAmpParams {
  falloffStartDistance: number;
  falloffEndDistance: number;
  exponent?: number;
  ampFactor: number;
}

export interface ReflectionParams {
  alpha: number;
}

/**
 * Used for determining default behavior like sound effects for when the player lands on a surface
 */
export enum MaterialClass {
  Default,
  Rock,
  Crystal,
  Instakill,
}

export interface CustomShaderProps {
  name?: string;
  side?: THREE.Side;
  roughness?: number;
  metalness?: number;
  clearcoat?: number;
  clearcoatRoughness?: number;
  clearcoatNormalMap?: THREE.Texture;
  clearcoatNormalScale?: number;
  iridescence?: number;
  sheen?: number;
  sheenColor?: THREE.Color | number;
  sheenRoughness?: number;
  color?: number | THREE.Color;
  normalScale?: number;
  map?: THREE.Texture;
  normalMap?: THREE.Texture;
  roughnessMap?: THREE.Texture;
  metalnessMap?: THREE.Texture;
  normalMapType?: THREE.NormalMapTypes;
  /**
   * If set to `true`, an attribute called `displacementNormal` is expected to be set on the geometry.
   *
   * These normals will be used instead of the object normals for displacement mapping.  This is useful
   * if you want to do flat/partially flat shading but still want to use displacement mapping.  If flat
   * shading is used and the object normals are used for displacement mapping, faces tend to fly apart
   * from each other.
   */
  useDisplacementNormals?: boolean;
  uvTransform?: THREE.Matrix3;
  emissiveIntensity?: number;
  lightMap?: THREE.Texture;
  lightMapIntensity?: number;
  transparent?: boolean;
  opacity?: number;
  alphaTest?: number;
  transmission?: number;
  ior?: number;
  transmissionMap?: THREE.Texture;
  fogMultiplier?: number;
  /**
   * If provided, maps will no longer be read once the fragment is this distance from the camera. Set to
   * `null` to disable.
   */
  mapDisableDistance?: number | null;
  /**
   * If provided, the shader will interpolate between read map value and diffuse color within this distance.
   */
  mapDisableTransitionThreshold?: number;
  /**
   * If greater than 0, fog will be darkened by shadows by this amount. A value of 1 means that the fog color
   * of a fully shadowed fragment will be darkened to the shadow color completely.
   */
  fogShadowFactor?: number;
  ambientLightScale?: number;
  /**
   * Controls an effect whereby the amount of ambient light is increased if the fragment is within some distance
   * to the camera.
   *
   * Works in a similar way to exp2 fog but in reverse and with a configurable exponent.
   */
  ambientDistanceAmp?: AmbientDistanceAmpParams;
  /**
   * Controls screen-space reflections.
   */
  reflection?: Partial<ReflectionParams>;
}

export interface CustomShaderShaders {
  customVertexFragment?: string;
  colorShader?: string;
  normalShader?: string;
  roughnessShader?: string;
  roughnessReverseColorRamp?: ReverseColorRampParams;
  metalnessShader?: string;
  metalnessReverseColorRamp?: ReverseColorRampParams;
  emissiveShader?: string;
  iridescenceShader?: string;
  iridescenceReverseColorRamp?: ReverseColorRampParams;
  displacementShader?: string;
  includeNoiseShadersVertex?: boolean;
}

export interface CustomShaderOptions {
  antialiasColorShader?: boolean;
  antialiasRoughnessShader?: boolean;
  tileBreaking?: { type: 'neyret'; patchScale?: number } | { type: 'fastFixMipmap' };
  /**
   * If set, the alternative noise functions in `noise2.frag` will be included
   */
  useNoise2?: boolean;
  enableFog?: boolean;
  /**
   * If set, a normal map will be generated based on derivatives in magnitude of generated diffuse colors.
   *
   * Note that this is a pretty broken implementation right now. There are huge aliasing issues and it looks
   * very bad on surfaces that have a high angle.
   */
  useComputedNormalMap?: boolean;
  /**
   * If set, the provided `map` will be treated as a combined grayscale diffuse + normal map. The diffuse
   * component will be read from the R channel and the normal map will be read from the GBA channels.
   */
  usePackedDiffuseNormalGBA?: boolean | { lut: Uint8Array<ArrayBuffer> };
  readRoughnessMapFromRChannel?: boolean;
  disableToneMapping?: boolean;
  // TODO: This is a shocking hack and should be removed
  disabledDirectionalLightIndices?: number[];
  disabledSpotLightIndices?: number[];
  randomizeUVOffset?: boolean;
  /**
   * Enabling this option will cause UV coordinates to be generated for this object using object space position and normal.
   *
   * This works very well for flat surfaces and simple geometries like boxes. For more complex objects, triplanar mapping
   * is a better alternative.
   */
  useGeneratedUVs?: boolean;
  /**
   * When set alongside `useGeneratedUVs`, UV coordinates are generated from world-space position instead of
   * object-space position.
   *
   * This is necessary for LOD terrain where each tile mesh has a unique local origin — without it, the same
   * world-space point yields different UVs in different tiles, causing visible texture popping on LOD transitions.
   *
   * **Constraint**: do not use this on animated/moving geometry. World-space UV generation causes textures to
   * slide across the surface as the object moves, which is usually undesirable.
   */
  useWorldSpaceGeneratedUVs?: boolean;
  useTriplanarMapping?: boolean | Partial<TriplanarMappingParams>;
  /**
   * Material class controls things like the sfx that are played when players land on the surface and
   * may also impact physics or other behavior in the future.
   */
  materialClass?: MaterialClass;
  /**
   * If true, the soft camera occlusion dither effect will NOT be applied to this material.
   * Use this for meshes that should always be fully visible (e.g. the player character).
   * Also sets `userData.occlusionExclude = true` on the built material so the depth pre-pass
   * can identify and skip these objects.
   */
  noOcclusion?: boolean;
}
