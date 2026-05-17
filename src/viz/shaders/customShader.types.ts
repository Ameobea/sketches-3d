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
  MetalPlate,
}

/**
 * Maps each MaterialClass variant to its canonical string name.
 * `satisfies Record<MaterialClass, string>` ensures the map stays exhaustive —
 * adding a new enum variant without updating this object is a compile error.
 */
export const MATERIAL_CLASS_NAMES = {
  [MaterialClass.Default]: 'default',
  [MaterialClass.Rock]: 'rock',
  [MaterialClass.Crystal]: 'crystal',
  [MaterialClass.Instakill]: 'instakill',
  [MaterialClass.MetalPlate]: 'metalplate',
} as const satisfies Record<MaterialClass, string>;

export type MaterialClassName = (typeof MATERIAL_CLASS_NAMES)[MaterialClass];

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
   * Which axes to use when computing the distance for `mapDisableDistance`.
   *
   * - `'xyz'` (default) — full 3D Euclidean distance.
   * - `'xz'`           — horizontal-only distance (ignores vertical offset), useful when
   *                       combined with XZ-only fog so that the texture fade matches the fog fade.
   */
  mapDisableDistanceAxes?: 'xyz' | 'xz';
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
  /**
   * Fades the fragment's alpha to zero based on world-space Y position. Auto-sets
   * `transparent: true` on the material so alpha blending is actually applied.
   *
   * `bottomFade: [fadeStart, fadeEnd]` — alpha ramps from 0 (at fadeStart) to 1 (at fadeEnd).
   * `topFade: [fadeStart, fadeEnd]`    — alpha ramps from 1 (at fadeStart) to 0 (at fadeEnd).
   *
   * Both are optional; omit either to skip that direction.
   */
  heightAlpha?: {
    bottomFade?: [fadeStart: number, fadeEnd: number];
    topFade?: [fadeStart: number, fadeEnd: number];
  };
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
  /**
   * GLSL defining `float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds)`,
   * the procedural height field for Parallax Occlusion Mapping.
   *
   * Returns the carved depth in `[0, 1]` where `0` = the base (undisplaced)
   * surface and `1` = a full `pom.depth` units carved inward along `-normal`.
   * `pos` is the world-space sample position; `normal` is the base surface
   * normal. The marcher owns the world-unit depth scaling so this function
   * stays dimensionless and reusable.
   *
   * Requires `opts.pom` to be set, and `useTriplanarMapping` in world space.
   * Mutually exclusive with `normalShader` (both fully define `normal`).
   */
  pomHeightShader?: string;
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
   * If set, the provided `map` will be treated as a combined grayscale diffuse + normal map. The diffuse
   * component will be read from the R channel and the normal map will be read from the GBA channels.
   */
  usePackedDiffuseNormalGBA?: boolean | { lut: Uint8Array<ArrayBuffer> };
  readRoughnessMapFromRChannel?: boolean;
  disableToneMapping?: boolean;
  // TODO: This is a shocking hack and should be removed
  disabledDirectionalLightIndices?: number[];
  disabledSpotLightIndices?: number[];
  /**
   * If true, applies a stable per-mesh random offset to texture sampling. Works with both
   * `useGeneratedUVs` (offsets `vUv`) and `useTriplanarMapping` (offsets the 3D sample position).
   *
   * The random key is sourced from a per-mesh uniform pushed via `onBeforeRender`, defaulting
   * to a hash of the mesh's id (or `mesh.userData.uvOffsetSeed` if explicitly set). Unlike the
   * old transform-based hash, this is stable across animation/movement.
   *
   * For meshes built outside the level-JSON pipeline, call `attachRandomizedUVOffset(mesh)`
   * once after assigning the material so the seed gets pushed each render.
   */
  randomizeUVOffset?: boolean;
  /**
   * Enabling this option will cause UV coordinates to be generated for this object using object space position and normal.
   *
   * This works very well for flat surfaces and simple geometries like boxes. For more complex objects, triplanar mapping
   * is a better alternative.
   */
  useGeneratedUVs?: boolean;
  /**
   * Selects the coordinate space used for UV generation.
   *
   * Applies to both `useGeneratedUVs` (drives the projected UV) and `useTriplanarMapping`
   * (drives the 3D sample position and the weighting normal).
   *
   * Defaults are mode-dependent to preserve legacy behavior when unset:
   *   - `useGeneratedUVs`: defaults to **local-space** (`false`).
   *   - `useTriplanarMapping`: defaults to **world-space** (`true`).
   *
   * World-space is necessary for LOD terrain where each tile mesh has a unique local origin
   * — without it, the same world-space point yields different UVs in different tiles, causing
   * visible texture popping on LOD transitions.
   *
   * **Constraint**: do not use world-space on animated/moving geometry. Textures will slide
   * across the surface as the object moves, which is usually undesirable.
   */
  useWorldSpaceUVs?: boolean;
  useTriplanarMapping?: boolean | Partial<TriplanarMappingParams>;
  /**
   * Enables procedural Parallax Occlusion Mapping (POM): the fragment shader
   * raymarches a procedural height field (supplied via `shaders.pomHeightShader`)
   * in world space and shades the displaced hit point, giving the illusion of
   * carved micro-geometry without extra tesselation.
   *
   * MVP constraints (validated at build time):
   *   - requires `shaders.pomHeightShader`
   *   - requires `useTriplanarMapping` in world space (the displaced hit
   *     position is fed back through the triplanar samplers 1:1)
   *   - cannot be combined with `normalShader` (POM owns the normal)
   *
   * Heightfield-only by default (ray clamps to the slab floor, silhouettes
   * unchanged). Opt into subtractive silhouette carving via `boundedSilhouette`
   * below. Technique, prior-art citations, limitations and authoring guidance:
   * `pom-known-limitations-and-authoring-guide.md` (repo root); formulation
   * rationale in `pom-implementation-plan.md` Appendix A.
   */
  pom?: {
    /** Max carve depth in world units. Also scales the analytic normal. */
    depth: number;
    /** Linear search step count. Default 24. Becomes a shader `#define`. */
    steps?: number;
    /**
     * Distance (world units) at which POM begins fading to the flat base
     * surface, over `lodFadeRange` units, to suppress sub-pixel aliasing
     * (brief §4). Defaults are derived from `depth` if omitted.
     */
    lodFadeStart?: number;
    lodFadeRange?: number;
    /**
     * **Opt-in, prototype.** Enables subtractive *silhouette* carving for
     * **convex** meshes via the back-face-depth-bounded relief formulation
     * (see `pom-silhouette-research-direction.md` / `pom-implementation-plan.md`
     * Appendix A).
     *
     * The Phase-1 floored core is reused byte-identical for the interior;
     * additionally the raymarch is bounded by the mesh's own nearest back face
     * (the convex exit). Where the local chord is thinner than the carve depth
     * (only near the silhouette) the ray exits the body before reaching the
     * carved floor and the fragment is `discard`ed, so the silhouette recedes
     * by exactly the groove depth — never expands (subtractive only).
     *
     * The caller MUST assert the mesh is convex and MUST supply the per-pixel
     * back-face exit distance via the `pomBackDepth` sampler uniform: a render
     * target holding the **Euclidean distance from the camera to the nearest
     * back face**, rendered with a front-face-culled depth-style pass of the
     * POM meshes (R channel, large sentinel where there is no back face).
     *
     * Concave/instanced/skinned/transparent meshes are out of scope and will
     * show artifacts. `discard` reintroduces the depth-prepass-bypass /
     * explicit-LOD-triplanar concerns from the phased plan §3.6/§4 — not yet
     * addressed at the prototype stage.
     */
    boundedSilhouette?: boolean;
  };
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
  /**
   * Enables retro-style vertex lighting (Gouraud shading). Lighting is evaluated per-vertex in
   * the vertex shader and interpolated across fragments, giving a classic faceted/low-poly look.
   *
   * Shadow maps are still sampled per-fragment for crisp shadow edges (hybrid approach).
   *
   * Incompatible with clearcoat, iridescence, sheen, and transmission — those are PBR fragment
   * effects that have no vertex-lighting equivalent.
   *
   * Normal maps have no effect on lighting when this is enabled (lighting uses geometric normals).
   */
  vertexLighting?: boolean;
  /**
   * Shininess exponent for Blinn-Phong specular highlights in vertex lighting mode.
   * Higher values produce tighter highlights; lower values produce a broad sheen.
   * Only has an effect when `vertexLighting` is enabled. Set to 0 or omit to disable specular.
   * Typical range: 8 (very broad) to 128 (tight pinpoint). Default: 0 (no specular).
   */
  vertexLightingShininess?: number;
}
