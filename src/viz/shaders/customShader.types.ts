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

/**
 * Used for determining default behavior like sound effects for when the player lands on a surface
 */
export enum MaterialClass {
  Default,
  Rock,
  Crystal,
  MetalPlate,
}

export const MATERIAL_CLASS_NAMES = {
  [MaterialClass.Default]: 'default',
  [MaterialClass.Rock]: 'rock',
  [MaterialClass.Crystal]: 'crystal',
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
  /**
   * Optional heightmap texture sampled during Parallax Occlusion Mapping.
   *
   * Sampled value (read from the R channel, in `[0,1]`) is interpreted as the
   * carved depth at that texel — `0` is the base surface, `1` is a full
   * `pom.depth` carved inward. Sampled with the same UV scheme as the rest of
   * the material (triplanar or generated UVs), at each marcher sample.
   *
   * Requires `opts.pom` and a non-baseline `pomTexturing` (i.e. either
   * `useTriplanarMapping` or `useGeneratedUVs`); the baseline path has no
   * analytic UV frame at the displaced sample point.
   *
   * Combined additively with `shaders.pomHeightShader` if both are provided;
   * the sum is clamped to `[0, 0.8]` before scaling by `pom.depth`.
   */
  pomHeightMap?: THREE.Texture;
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
  /** Prefiltered (PMREM) env texture; falls back to the scene env when omitted. */
  envMap?: THREE.Texture;
  envMapIntensity?: number;
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
   * Darkens the fragment toward black based on world-space Y position. Despite the name, this
   * does NOT touch alpha — true alpha fading proved too hard to make look good, so the
   * implementation fades the output color to black instead (works well against dark fog/sky).
   *
   * `bottomFade: [fadeStart, fadeEnd]` — fully black at fadeStart, unfaded at fadeEnd.
   * `topFade: [fadeStart, fadeEnd]`    — unfaded at fadeStart, fully black at fadeEnd.
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
  /**
   * GLSL emitted into the fragment shader before all other user shader slots
   * (`colorShader`, `pomHeightShader`, etc.) and after the engine helpers
   * (noise, triplanar, generated-UV). Use it to declare structs and helper
   * functions shared by multiple slots so the logic lives in one place.
   *
   * Everything lands in a single translation unit with shared global scope,
   * so anything declared here is visible to every other slot.
   */
  commonShader?: string;
  colorShader?: string;
  /**
   * GLSL defining `vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx)`,
   * returning `(directMul, indirectMul)` in `[0,1]`. Applied after `<lights_fragment_end>`,
   * scaling `reflectedLight.direct{Diffuse,Specular}` and `…indirect{Diffuse,Specular}`
   * respectively — for procedural shadows / AO that must dim the actual light (specular
   * included), not just the albedo a `colorShader` reaches. Under POM `pos`/`normal` are
   * the displaced hit + relief normal.
   */
  lightAttenuationShader?: string;
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
   * normal.
   *
   * Requires `opts.pom` to be set, and at least one of this or
   * `props.pomHeightMap` must be provided. If both are provided, their values
   * are added together (then clamped to `[0, 0.8]`). Mutually exclusive with
   * `normalShader` (both fully define `normal`). See `pom` for the texturing
   * modes.
   */
  pomHeightShader?: string;
  /**
   * GLSL defining `vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t)`,
   * a closed-form world-space normal for the carved POM floor. Replaces the
   * engine's finite-difference normal (several extra `getPomHeight` evals per
   * fragment) with one analytic call.
   *
   * Requires `pomHeightShader`. Only for procedural-height materials with no
   * `pomHeightMap` — an analytic gradient can't see a heightmap's contribution.
   */
  pomNormalShader?: string;
  includeNoiseShadersVertex?: boolean;
}

export interface CustomShaderOptions {
  antialiasColorShader?: boolean;
  antialiasRoughnessShader?: boolean;
  tileBreaking?: { type: 'neyret'; patchScale?: number };
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
   * `useGeneratedUVs` (offsets `vUv`) and `useTriplanarMapping`.
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
   */
  useWorldSpaceUVs?: boolean;
  useTriplanarMapping?: boolean | Partial<TriplanarMappingParams>;
  /**
   * Enables procedural Parallax Occlusion Mapping.  The fragment shader
   * raymarches a height field and shades the displaced hit point, giving the
   * illusion of carved or inset geometry without extra tesselation.
   *
   * Requires at least one of `shaders.pomHeightShader` (procedural) or
   * `props.pomHeightMap` (heightmap texture).
   */
  pom?: {
    /** Max carve depth in world units. Also scales the analytic normal. */
    depth: number;
    /** Linear search step count. Default 24. */
    steps?: number;
    /**
     * Offsets each fragment's march phase by interleaved gradient noise, converting
     * sample-interval banding into unstructured per-pixel noise (which the dithering
     * pipeline already hides well). Often allows reducing `steps` at equal perceived
     * quality. Applies to the linear, bounded, and self-shadow marches. Default true.
     */
    jitter?: boolean;
    /**
     * Distance (world units) at which POM begins fading to the flat base
     * surface, over `lodFadeRange` units, to suppress sub-pixel aliasing.
     * Defaults are derived from `depth` if omitted.
     */
    lodFadeStart?: number;
    lodFadeRange?: number;
    /**
     * Enables subtractive silhouette carving via the back-face-depth-bounded relief formulation.
     *
     * Only works well on convex meshes.
     */
    boundedSilhouette?: boolean;
    /**
     * Applies the carved relief's per-pixel normal (`_pomNormalW`) to the shading
     * normal so the inset walls catch direct light and specular even without a
     * `normalMap`. Off by default: materials that lean on flat-normal lighting
     * plus a color-shader pseudo-AO (e.g. the masonry tiles) would otherwise
     * change appearance. No effect when a `normalMap` is set — that path already
     * folds the relief normal into the shading normal.
     */
    applyReliefNormal?: boolean;
    /**
     * Opt-in relief self-shadowing: marches the height field from the displaced
     * hit toward an explicit light direction and darkens the direct lighting
     * where the relief occludes it. Independent of the scene's lights — the
     * direction is set here. Off by default.
     */
    selfShadow?: {
      /** World-space direction toward the light (normalized internally). */
      lightDir: [number, number, number];
      /** March step count. Default 12. */
      steps?: number;
      /** Shadow opacity in `[0,1]`. Default 1. */
      strength?: number;
      /** Penumbra softness; carved fraction giving full local occlusion. Default 0.5. */
      softness?: number;
    };
    /**
     * Refinement after the linear search brackets the surface.
     * - `"secant"` (default): one-step linear interp. ~Free; ideal for smooth heightfields.
     * - `"binary"`: bisection over `refinementSteps`. Use on cliffs/step
     *   heightfields where secant produces sample-interval banding.
     */
    refinement?: 'secant' | 'binary';
    /** Bisection count when `refinement: "binary"`. Default 5; 4–6 is the sweet spot. */
    refinementSteps?: number;
    /**
     * `refinement: "binary"` only. Skip bisection when the linear search already
     * pierced the floor by less than this fraction of one march step (measured in
     * depth, where each step advances `depth / steps`). Reuses the free secant
     * root in that case, eliminating the fixed refinement evals on the common
     * "hit just past the surface" fragments while still bisecting the deep-plunge
     * (steep wall / step) fragments where refinement is load-bearing.
     *
     * Safe on step heightfields: a small overshoot drives the secant weight to 0,
     * collapsing the result onto the linear hit. Higher = more aggressive skip
     * (more perf, more sample-interval error on walls). `0` disables (compiles out
     * to the original always-bisect path). Default `0.5`.
     */
    refineSkipThreshold?: number;
    /**
     * World-space floor for the analytic-normal finite-difference radius
     * `_pomEps`. Effective `_pomEps = max(unitsPerPx, pomDepth * 0.02, normalEps)`.
     *
     * Use this with `pomHeightMap` to widen the gradient kernel beyond a single
     * texel — without this floor the 4 taps land on adjacent texels with very
     * different heights and the resulting normal is per-pixel noise. A good
     * starting value is 3–4× the texel size in world units, i.e. roughly
     * `4 / (uvScale * heightmapResolution)`. Default: unset (no floor).
     */
    normalEps?: number;
    /**
     * Replaces the final fragment color with a diagnostic visualization of an
     * intermediate POM quantity. Bypasses the color shader, normal map, and
     * fog so the value lands on screen untouched (tonemapping still applies).
     *
     * - `'heightmap'`   — raw `samplePomHeightMap(_pomHit, vWorldNormal)` as grayscale
     * - `'depth'`       — final `_pomSurf / pomDepth` (combined procedural + map carved depth)
     * - `'normal'`      — `_pomNormalW * 0.5 + 0.5` (perturbed shading normal, world space)
     * - `'normalDelta'` — `length(_pomNormalW - vWorldNormal)` as red (size of perturbation)
     * - `'axis'`        — triplanar dominant-axis pick of `_pomNormalW` (R=X, G=Y, B=Z).
     *                     Per-pixel speckle here is the smoking gun for heightmap-driven
     *                     gradient noise flipping triplanar axes.
     * - `'hit'`         — `fract(_pomHit)` as RGB (sanity-check displacement spread)
     * - `'samples'`     — per-fragment height-field eval count (linear march +
     *                     binary refine + analytic-normal taps) as a heat map
     *                     (blue = cheap → red = worst case). Estimated march cost.
     * - `'skip'`        — refinement decision (binary refinement only): green =
     *                     bisection skipped (linear hit accepted), red = full
     *                     bisection ran, dark blue = no refinement reached.
     */
    debug?: 'heightmap' | 'depth' | 'normal' | 'normalDelta' | 'axis' | 'hit' | 'samples' | 'skip';
  };
  /**
   * Material class controls things like the sfx that are played when players land on the surface and
   * may also impact physics or other behavior in the future.
   */
  materialClass?: MaterialClass;
  /**
   * If true, the soft camera occlusion dither effect will NOT be applied to this material.
   * Use this for meshes that should always be fully visible (e.g. the player character).
   */
  noOcclusion?: boolean;
  /**
   * Enables retro-style vertex lighting (Gouraud shading). Lighting is evaluated per-vertex in
   * the vertex shader and interpolated across fragments, giving a classic faceted/low-poly look.
   *
   * Shadow maps are still sampled per-fragment for crisp shadow edges.
   *
   * Incompatible with clearcoat, iridescence, sheen, and transmission — those are PBR fragment
   * effects that have no vertex-lighting equivalent.
   *
   * Normal maps have no effect on lighting when this is enabled.
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
