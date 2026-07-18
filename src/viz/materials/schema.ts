import { z } from 'zod';

import { MATERIAL_CLASS_NAMES, type MaterialClassName } from 'src/viz/shaders/customShader.types';

const Vec2 = z.tuple([z.number(), z.number()]);
const Vec3 = z.tuple([z.number(), z.number(), z.number()]);

// Extract values from the enum→name map as a non-empty tuple for z.enum
const materialClassNameValues = Object.values(MATERIAL_CLASS_NAMES) as [
  MaterialClassName,
  ...MaterialClassName[],
];

export const AmbientDistanceAmpParamsSchema = z.object({
  falloffStartDistance: z.number(),
  falloffEndDistance: z.number(),
  exponent: z.number().optional(),
  ampFactor: z.number(),
});

export const TriplanarMappingParamsJsonSchema = z.object({
  contrastPreservationFactor: z.number().optional(),
  sharpenFactor: z.number().optional(),
});

export const ReverseColorRampParamsSchema = z.object({
  colorA_srgb: Vec3,
  colorB_srgb: Vec3,
  vMin: z.number(),
  vMax: z.number(),
  curveSteepness: z.number(),
  curveOffset: z.number(),
  perpSigma: z.number(),
  baseFallback: z.number(),
  colorSpace: z.enum(['srgb', 'linear']).optional(),
});

/**
 * Serializable form of `CustomUniformDef` (see customShader.types). A `sampler2D` value is a
 * string key into the textures registry (in geotoy-format defs, a direct texture URL).
 * Vector/matrix values are flat number arrays in column-major order (matching THREE's `toArray`).
 */
export const CustomUniformJsonSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('float'), value: z.number(), vertex: z.boolean().optional() }),
  z.object({ type: z.literal('sampler2D'), value: z.string(), vertex: z.boolean().optional() }),
  z.object({ type: z.literal('int'), value: z.number().int(), vertex: z.boolean().optional() }),
  z.object({ type: z.literal('vec2'), value: Vec2, vertex: z.boolean().optional() }),
  z.object({ type: z.literal('vec3'), value: Vec3, vertex: z.boolean().optional() }),
  z.object({
    type: z.literal('vec4'),
    value: z.tuple([z.number(), z.number(), z.number(), z.number()]),
    vertex: z.boolean().optional(),
  }),
  z.object({ type: z.literal('mat3'), value: z.array(z.number()).length(9), vertex: z.boolean().optional() }),
  z.object({
    type: z.literal('mat4'),
    value: z.array(z.number()).length(16),
    vertex: z.boolean().optional(),
  }),
]);
export type CustomUniformJson = z.infer<typeof CustomUniformJsonSchema>;

/**
 * Serializable form of `ShaderConstantDef` (see customShader.types). Baked into the fragment GLSL
 * as a `#define`; the material's slot GLSL guards its default with `#ifndef` and a value here
 * overrides it. Compile-time, so usable in const-expressions/array-sizes a uniform can't reach.
 */
export const ShaderConstantJsonSchema = z.discriminatedUnion('type', [
  z.object({ type: z.literal('float'), value: z.number() }),
  z.object({ type: z.literal('int'), value: z.number().int() }),
  z.object({ type: z.literal('bool'), value: z.boolean() }),
  z.object({ type: z.literal('vec2'), value: Vec2 }),
  z.object({ type: z.literal('vec3'), value: Vec3 }),
  z.object({ type: z.literal('vec4'), value: z.tuple([z.number(), z.number(), z.number(), z.number()]) }),
]);
export type ShaderConstantJson = z.infer<typeof ShaderConstantJsonSchema>;

export const ShaderShadersJsonSchema = z.object({
  customVertexFragment: z.string().optional(),
  /** Shared GLSL emitted before all other user shader slots; see `CustomShaderShaders.commonShader`. */
  commonShader: z.string().optional(),
  colorShader: z.string().optional(),
  lightAttenuationShader: z.string().optional(),
  normalShader: z.string().optional(),
  roughnessShader: z.string().optional(),
  roughnessReverseColorRamp: ReverseColorRampParamsSchema.optional(),
  metalnessShader: z.string().optional(),
  metalnessReverseColorRamp: ReverseColorRampParamsSchema.optional(),
  emissiveShader: z.string().optional(),
  iridescenceShader: z.string().optional(),
  iridescenceReverseColorRamp: ReverseColorRampParamsSchema.optional(),
  displacementShader: z.string().optional(),
  includeNoiseShadersVertex: z.boolean().optional(),
  pomHeightShader: z.string().optional(),
  pomNormalShader: z.string().optional(),
  customUniforms: z.record(z.string(), CustomUniformJsonSchema).optional(),
  constants: z.record(z.string(), ShaderConstantJsonSchema).optional(),
});

/** Texture-bearing slots on `CustomShaderMatDef.props`; values are texture-registry keys. */
export const TEXTURE_SLOTS = [
  'map',
  'normalMap',
  'roughnessMap',
  'metalnessMap',
  'lightMap',
  'transmissionMap',
  'clearcoatNormalMap',
  'pomHeightMap',
] as const;
export type TextureSlot = (typeof TEXTURE_SLOTS)[number];

/** Serializable CustomShaderProps — texture slots as string keys into textures registry */
export const ShaderPropsJsonSchema = z.object({
  color: z.number().optional(),
  roughness: z.number().optional(),
  metalness: z.number().optional(),
  normalScale: z.number().optional(),
  emissiveIntensity: z.number().optional(),
  lightMapIntensity: z.number().optional(),
  envMapIntensity: z.number().optional(),
  opacity: z.number().optional(),
  alphaTest: z.number().optional(),
  transparent: z.boolean().optional(),
  transmission: z.number().optional(),
  ior: z.number().optional(),
  clearcoat: z.number().optional(),
  clearcoatRoughness: z.number().optional(),
  clearcoatNormalScale: z.number().optional(),
  iridescence: z.number().optional(),
  sheen: z.number().optional(),
  sheenColor: z.number().optional(),
  sheenRoughness: z.number().optional(),
  fogMultiplier: z.number().optional(),
  fogShadowFactor: z.number().optional(),
  ambientLightScale: z.number().optional(),
  mapDisableDistance: z.number().nullable().optional(),
  mapDisableDistanceAxes: z.union([z.literal('xyz'), z.literal('xz')]).optional(),
  mapDisableTransitionThreshold: z.number().optional(),
  side: z.enum(['front', 'back', 'double']).optional(),
  /**
   * Uniform UV scale applied as `uvTransform = Matrix3.scale(x, y)`.
   * Shorthand for the most common uvTransform use case.
   */
  uvScale: Vec2.optional(),
  // texture refs — string keys into textures registry
  map: z.string().optional(),
  normalMap: z.string().optional(),
  roughnessMap: z.string().optional(),
  metalnessMap: z.string().optional(),
  lightMap: z.string().optional(),
  transmissionMap: z.string().optional(),
  clearcoatNormalMap: z.string().optional(),
  /** Optional heightmap texture sampled during Parallax Occlusion Mapping. Requires `options.pom`. */
  pomHeightMap: z.string().optional(),
  // complex but JSON-safe
  ambientDistanceAmp: AmbientDistanceAmpParamsSchema.optional(),
  heightAlpha: z
    .object({
      bottomFade: Vec2.optional(),
      topFade: Vec2.optional(),
    })
    .optional(),
});

/** Serializable subset of CustomShaderOptions */
export const ShaderOptionsJsonSchema = z.object({
  useTriplanarMapping: z.union([z.boolean(), TriplanarMappingParamsJsonSchema]).optional(),
  useGeneratedUVs: z.boolean().optional(),
  useWorldSpaceUVs: z.boolean().optional(),
  tileBreaking: z.object({ type: z.literal('neyret'), patchScale: z.number().optional() }).optional(),
  enableFog: z.boolean().optional(),
  antialiasColorShader: z.boolean().optional(),
  antialiasRoughnessShader: z.boolean().optional(),
  readRoughnessMapFromRChannel: z.boolean().optional(),
  disableToneMapping: z.boolean().optional(),
  randomizeUVOffset: z.boolean().optional(),
  useNoise2: z.boolean().optional(),
  useOrenNayarDiffuse: z.boolean().optional(),
  materialClass: z.enum(materialClassNameValues).optional(),
  pom: z
    .object({
      depth: z.number(),
      tier: z.enum(['field', 'projectedField', 'grid']).optional(),
      cellPitch: z.number().positive().optional(),
      cellType: z.string().optional(),
      hitType: z.string().optional(),
      intersect: z.enum(['march', 'safeStep', 'analytic']).optional(),
      minFeatureWidth: z.number().positive().optional(),
      lateralDist: z.boolean().optional(),
      steps: z.number().int().min(1).optional(),
      jitter: z.boolean().optional(),
      lodFadeStart: z.number().optional(),
      lodFadeRange: z.number().optional(),
      boundedSilhouette: z.boolean().optional(),
      applyReliefNormal: z.boolean().optional(),
      tangentSpace: z.boolean().optional(),
      selfShadow: z
        .object({
          lightDir: z.tuple([z.number(), z.number(), z.number()]),
          steps: z.number().int().min(1).optional(),
          strength: z.number().min(0).optional(),
          softness: z.number().positive().optional(),
        })
        .optional(),
      refinement: z.enum(['secant', 'binary']).optional(),
      refinementSteps: z.number().int().min(1).optional(),
      refineSkipThreshold: z.number().min(0).optional(),
      normalEps: z.number().positive().optional(),
      debug: z
        .enum(['heightmap', 'depth', 'normal', 'normalDelta', 'axis', 'hit', 'samples', 'evals', 'skip'])
        .optional(),
    })
    .optional(),
});

export const BoostSurfaceConfigSchema = z.object({
  /** Ground walk-speed override while the aux key is held and the player stands on this surface. */
  targetSpeed: z.number(),
  /** 0..1; fraction of live walk velocity locked into external vel on jump from this surface. */
  jumpRetention: z.number(),
  /** Seconds to ramp from base walk speed up to `targetSpeed` after the aux key arms boost.
   *  Curve is a soft-knee smoothstep² (slow start, fast finish, soft top).  Default 0 = snap. */
  rampUpSeconds: z.number().optional(),
  /** If true (default), boost jump-retention launches along the floor's tangent plane (capped
   *  at ±45°) instead of purely horizontally — so a ramped strip launches you up the ramp. */
  followSurfaceSlope: z.boolean().optional(),
});
export type BoostSurfaceConfigDef = z.infer<typeof BoostSurfaceConfigSchema>;

export const ParkourMaterialMetaSchema = z.object({
  /** Default boost-surface config for objects using this material; an object's own
   *  `parkour.boostSurface` overrides this if set. */
  boostSurface: BoostSurfaceConfigSchema.optional(),
});
export type ParkourMaterialMeta = z.infer<typeof ParkourMaterialMetaSchema>;

/** Per-axis (0..1) damping factor for the in-air external-velocity term while the player is
 *  standing on (or just left) a surface using this material/object.  Higher = velocity bleeds
 *  off faster.  Either field may be omitted to fall back to the scene-level default for that
 *  axis-pair. */
export const ExternalVelocityDampingOverrideFields = {
  externalVelocityAirDampingFactor: z.tuple([z.number(), z.number(), z.number()]).optional(),
  externalVelocityGroundDampingFactor: z.tuple([z.number(), z.number(), z.number()]).optional(),
} as const;

/**
 * Recipe for the geoscript mesh UV-unwrap preprocessing step (geoscript runner → WASM unwrapper),
 * which writes the `uv` attribute the material then samples. Run before material build; `buildMaterial`
 * ignores it. Its presence distinguishes an unwrapped mesh from one whose existing `uv`s are used as-is.
 */
export const MeshUvUnwrapParamsSchema = z.object({
  numCones: z.number().int(),
  flattenToDisk: z.boolean(),
  mapToSphere: z.boolean(),
  enableUVIslandRotation: z.boolean(),
});
export type MeshUvUnwrapParams = z.infer<typeof MeshUvUnwrapParamsSchema>;

export const CustomShaderMatDefSchema = z.object({
  type: z.literal('customShader'),
  /** Geotoy display + geoscript-reference name; level-def keys materials by record name and ignores this. */
  name: z.string().optional(),
  props: ShaderPropsJsonSchema.optional(),
  options: ShaderOptionsJsonSchema.optional(),
  shaders: ShaderShadersJsonSchema.optional(),
  emissiveBypass: z.boolean().optional(),
  /** Two-output material: lit/tone-mapped base + all emissive (uniform/map/`emissiveShader`)
   *  routed to the bypass buffer. Requires a pipeline with `emissiveBypass: true`.
   *  See `CustomShaderOptions.inlineEmissiveBypass`. */
  inlineEmissiveBypass: z.boolean().optional(),
  /** If true, the camera will hard-snap rather than dither through this material, and the
   *  soft-occlusion shader effect is disabled for it. */
  nonPermeable: z.boolean().optional(),
  parkour: ParkourMaterialMetaSchema.optional(),
  /** Geoscript-only mesh UV-unwrap recipe (see `MeshUvUnwrapParamsSchema`); ignored by `buildMaterial`. */
  meshUvUnwrap: MeshUvUnwrapParamsSchema.optional(),
  ...ExternalVelocityDampingOverrideFields,
});

export const CustomBasicShaderMatDefSchema = z.object({
  type: z.literal('customBasicShader'),
  name: z.string().optional(),
  props: z
    .object({
      color: z.number().optional(),
      transparent: z.boolean().optional(),
      alphaTest: z.number().optional(),
      fogMultiplier: z.number().optional(),
    })
    .optional(),
  options: z
    .object({
      enableFog: z.boolean().optional(),
    })
    .optional(),
  emissiveBypass: z.boolean().optional(),
  /** If true, the camera will hard-snap rather than dither through this material. */
  nonPermeable: z.boolean().optional(),
  parkour: ParkourMaterialMetaSchema.optional(),
  ...ExternalVelocityDampingOverrideFields,
});

/**
 * A material whose `THREE.Material` instance is provided at runtime via `LevelLoadHandle.setMaterialFactories`.
 * Used for fully custom materials that can't be described in data (e.g. animated or shader-heavy materials).
 */
export const GeneratedMatDefSchema = z.object({
  type: z.literal('generated'),
  emissiveBypass: z.boolean().optional(),
  /** If true, the camera will hard-snap rather than dither through this material.
   *  The runtime-provided material must have `userData.nonPermeable = true` set manually. */
  nonPermeable: z.boolean().optional(),
  parkour: ParkourMaterialMetaSchema.optional(),
  ...ExternalVelocityDampingOverrideFields,
});

export const MaterialDefSchema = z.discriminatedUnion('type', [
  CustomShaderMatDefSchema,
  CustomBasicShaderMatDefSchema,
  GeneratedMatDefSchema,
]);

export type AmbientDistanceAmpParams = z.infer<typeof AmbientDistanceAmpParamsSchema>;
export type TriplanarMappingParamsJson = z.infer<typeof TriplanarMappingParamsJsonSchema>;
export type ReverseColorRampParamsJson = z.infer<typeof ReverseColorRampParamsSchema>;
export type ShaderShadersJson = z.infer<typeof ShaderShadersJsonSchema>;
export type ShaderPropsJson = z.infer<typeof ShaderPropsJsonSchema>;
export type ShaderOptionsJson = z.infer<typeof ShaderOptionsJsonSchema>;
export type CustomShaderMatDef = z.infer<typeof CustomShaderMatDefSchema>;
export type CustomBasicShaderMatDef = z.infer<typeof CustomBasicShaderMatDefSchema>;
export type GeneratedMatDef = z.infer<typeof GeneratedMatDefSchema>;
export type MaterialDef = z.infer<typeof MaterialDefSchema>;
