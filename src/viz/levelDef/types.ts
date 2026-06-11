import { z } from 'zod';

import { MATERIAL_CLASS_NAMES, type MaterialClassName } from 'src/viz/shaders/customShader.types';

// Extract values from the enum→name map as a non-empty tuple for z.enum
const materialClassNameValues = Object.values(MATERIAL_CLASS_NAMES) as [
  MaterialClassName,
  ...MaterialClassName[],
];

const Vec2 = z.tuple([z.number(), z.number()]);
const Vec3 = z.tuple([z.number(), z.number(), z.number()]);

/**
 * Accepts a color as either a plain integer (e.g. 0x303030 → 3158064) or a
 * CSS-style 6-digit hex string (e.g. "#303030").  No transform here so the
 * schema remains representable in JSON Schema; callers that need an integer
 * must run `normalizeRawDefColors` after parsing.
 */
const ColorInputSchema = z.union([
  z.number().int(),
  z.string().regex(/^#[0-9a-fA-F]{6}$/i, 'Color string must be a 6-digit hex value like "#rrggbb"'),
]);

/**
 * Collision shape type for an asset's collider.  Default: `trimesh` (exact triangle mesh,
 * with automatic box/sphere detection for simple primitives).  `convexHull` precomputes
 * a real convex hull (via Manifold) once per asset and uses that as the trimesh; useful
 * for shapes where concave accuracy isn't needed and a smooth bounding hull is preferred.
 *
 * Lives on the asset, not the object, so the (potentially expensive) hull is computed
 * exactly once per asset and shared across all instances.
 */
const AssetColliderShapeSchema = z.enum(['trimesh', 'convexHull']);
export type AssetColliderShape = z.infer<typeof AssetColliderShapeSchema>;

export const GltfAssetDefSchema = z.object({
  type: z.literal('gltf'),
  /** Name of the mesh/object as it appears in the loaded gltf scene */
  meshName: z.string(),
  colliderShape: AssetColliderShapeSchema.optional(),
});

export const GeoscriptAssetMetaSchema = z.object({
  runtimeMs: z.number(),
  count: z.number().int().min(0).max(5),
  codeHash: z.string(),
  /** Async dep names actually used during eval. Omitted when none were used. */
  asyncDeps: z.array(z.string()).optional(),
});
export type GeoscriptAssetMeta = z.infer<typeof GeoscriptAssetMetaSchema>;

export const GeoscriptAssetDefSchema = z.object({
  type: z.literal('geoscript'),
  code: z.string(),
  /** Whether to include the standard geoscript prelude. Default: true */
  includePrelude: z.boolean().optional(),
  colliderShape: AssetColliderShapeSchema.optional(),
  _meta: GeoscriptAssetMetaSchema.optional(),
});

/** Geoscript asset that references an external file (resolved server-side before the client sees it). */
export const GeoscriptAssetDefFileSchema = z.object({
  type: z.literal('geoscript'),
  /** Path to a .geo file, relative to the level's directory. */
  file: z.string(),
  /** Whether to include the standard geoscript prelude. Default: true */
  includePrelude: z.boolean().optional(),
  colliderShape: AssetColliderShapeSchema.optional(),
  _meta: GeoscriptAssetMetaSchema.optional(),
});

/** Union of both geoscript asset forms — used when parsing JSON from disk. */
export const GeoscriptAssetDefRawSchema = z.union([GeoscriptAssetDefSchema, GeoscriptAssetDefFileSchema]);

export interface CsgLeafNode {
  asset: string;
  position?: [number, number, number];
  rotation?: [number, number, number];
  scale?: [number, number, number];
}

export interface CsgOpNode {
  op: 'union' | 'difference' | 'intersection';
  children: CsgTreeNode[];
  position?: [number, number, number];
  rotation?: [number, number, number];
  scale?: [number, number, number];
}

export type CsgTreeNode = CsgLeafNode | CsgOpNode;

const CsgLeafNodeSchema: z.ZodType<CsgLeafNode> = z.object({
  asset: z.string(),
  position: Vec3.optional(),
  rotation: Vec3.optional(),
  scale: Vec3.optional(),
});

const CsgOpNodeSchema: z.ZodType<CsgOpNode> = z.lazy(() =>
  z.object({
    op: z.enum(['union', 'difference', 'intersection']),
    children: z.array(CsgTreeNodeSchema).min(2),
    position: Vec3.optional(),
    rotation: Vec3.optional(),
    scale: Vec3.optional(),
  })
);

const CsgTreeNodeSchema: z.ZodType<CsgTreeNode> = z.union([CsgOpNodeSchema, CsgLeafNodeSchema]);

export const CsgAssetDefSchema = z.object({
  type: z.literal('csg'),
  tree: CsgTreeNodeSchema,
  colliderShape: AssetColliderShapeSchema.optional(),
  _meta: GeoscriptAssetMetaSchema.optional(),
});

export type CsgAssetDef = z.infer<typeof CsgAssetDefSchema>;

/** Return type after server-side inlining: always has `code`. */
export const AssetDefSchema = z.discriminatedUnion('type', [
  GltfAssetDefSchema,
  GeoscriptAssetDefSchema,
  CsgAssetDefSchema,
]);

/** Input type for JSON files on disk: geoscript assets may have `code` or `file`. */
export const AssetDefRawSchema = z.union([
  GltfAssetDefSchema,
  GeoscriptAssetDefSchema,
  GeoscriptAssetDefFileSchema,
  CsgAssetDefSchema,
]);

export type GltfAssetDef = z.infer<typeof GltfAssetDefSchema>;
export type GeoscriptAssetDef = z.infer<typeof GeoscriptAssetDefSchema>;
export type GeoscriptAssetDefFile = z.infer<typeof GeoscriptAssetDefFileSchema>;
export type GeoscriptAssetDefRaw = z.infer<typeof GeoscriptAssetDefRawSchema>;
export type AssetDef = z.infer<typeof AssetDefSchema>;
export type AssetDefRaw = z.infer<typeof AssetDefRawSchema>;

export const TextureDefSchema = z.object({
  url: z.string(),
  /** Default: 'repeat' */
  wrapS: z.enum(['repeat', 'clamp', 'mirror']).optional(),
  /** Default: 'repeat' */
  wrapT: z.enum(['repeat', 'clamp', 'mirror']).optional(),
  /** Default: 'nearest' */
  magFilter: z.enum(['nearest', 'linear']).optional(),
  /** Default: 'nearestMipLinear' */
  minFilter: z.enum(['nearest', 'nearestMipNearest', 'nearestMipLinear', 'linearMipLinear']).optional(),
  /** Default: 1 */
  anisotropy: z.number().optional(),
  /** Default: '' (`NoColorSpace`) */
  colorSpace: z.enum(['srgb', '']).optional(),
  /**
   * Default: 'rgba' (`RGBA8`)
   */
  format: z.enum(['rgba', 'rg', 'red']).optional(),
});

export type TextureDef = z.infer<typeof TextureDefSchema>;

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
  materialClass: z.enum(materialClassNameValues).optional(),
  pom: z
    .object({
      depth: z.number(),
      steps: z.number().int().min(1),
      jitter: z.boolean().optional(),
      lodFadeStart: z.number().optional(),
      lodFadeRange: z.number().optional(),
      boundedSilhouette: z.boolean().optional(),
      applyReliefNormal: z.boolean().optional(),
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
        .enum(['heightmap', 'depth', 'normal', 'normalDelta', 'axis', 'hit', 'samples', 'skip'])
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

export const CustomShaderMatDefSchema = z.object({
  type: z.literal('customShader'),
  props: ShaderPropsJsonSchema.optional(),
  options: ShaderOptionsJsonSchema.optional(),
  shaders: ShaderShadersJsonSchema.optional(),
  emissiveBypass: z.boolean().optional(),
  /** If true, the camera will hard-snap rather than dither through this material, and the
   *  soft-occlusion shader effect is disabled for it. */
  nonPermeable: z.boolean().optional(),
  parkour: ParkourMaterialMetaSchema.optional(),
  ...ExternalVelocityDampingOverrideFields,
});

export const CustomBasicShaderMatDefSchema = z.object({
  type: z.literal('customBasicShader'),
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

/** Like ShaderPropsJsonSchema but `color` and `sheenColor` accept "#rrggbb" strings. */
const ShaderPropsJsonRawSchema = ShaderPropsJsonSchema.extend({
  color: ColorInputSchema.optional(),
  sheenColor: ColorInputSchema.optional(),
});

/** A GLSL shader snippet as an inline string or a file reference resolved server-side. */
const ShaderGlslFieldRawSchema = z.union([z.string(), z.object({ file: z.string() })]);

/** Like ShaderShadersJsonSchema but GLSL string fields also accept `{ file: string }`. */
const ShaderShadersJsonRawSchema = ShaderShadersJsonSchema.extend({
  customVertexFragment: ShaderGlslFieldRawSchema.optional(),
  commonShader: ShaderGlslFieldRawSchema.optional(),
  colorShader: ShaderGlslFieldRawSchema.optional(),
  lightAttenuationShader: ShaderGlslFieldRawSchema.optional(),
  normalShader: ShaderGlslFieldRawSchema.optional(),
  roughnessShader: ShaderGlslFieldRawSchema.optional(),
  metalnessShader: ShaderGlslFieldRawSchema.optional(),
  emissiveShader: ShaderGlslFieldRawSchema.optional(),
  iridescenceShader: ShaderGlslFieldRawSchema.optional(),
  displacementShader: ShaderGlslFieldRawSchema.optional(),
  pomHeightShader: ShaderGlslFieldRawSchema.optional(),
  pomNormalShader: ShaderGlslFieldRawSchema.optional(),
});

const CustomShaderMatDefRawSchema = CustomShaderMatDefSchema.extend({
  props: ShaderPropsJsonRawSchema.optional(),
  shaders: ShaderShadersJsonRawSchema.optional(),
});

const CustomBasicShaderMatDefRawSchema = CustomBasicShaderMatDefSchema.extend({
  props: z
    .object({
      color: ColorInputSchema.optional(),
      transparent: z.boolean().optional(),
      alphaTest: z.number().optional(),
      fogMultiplier: z.number().optional(),
    })
    .optional(),
});

export const MaterialDefRawSchema = z.discriminatedUnion('type', [
  CustomShaderMatDefRawSchema,
  CustomBasicShaderMatDefRawSchema,
  GeneratedMatDefSchema,
]);
export type MaterialDefRaw = z.infer<typeof MaterialDefRawSchema>;

/**
 * File shape for a shared-library material at `src/assets/materials/<sub>/<name>.json`.
 * Referenced from a level def as `material: "__ASSETS__/materials/<sub>/<name>"`.
 *
 * `textures` are local to this file; on import their keys are prefixed with the
 * library path so they can't collide with the consuming level's textures.
 */
export const LibraryMaterialFileSchema = z.object({
  $schema: z.string().optional(),
  textures: z.record(z.string(), TextureDefSchema).optional(),
  material: MaterialDefRawSchema,
});
export type LibraryMaterialFile = z.infer<typeof LibraryMaterialFileSchema>;

const HEX_COLOR_RE = /^#[0-9a-fA-F]{6}$/i;

const COLOR_KEYS = new Set(['color', 'sheenColor', 'skyColor', 'groundColor', 'horizonColor']);

/** Recursively walk a plain JSON object and convert hex color strings to integers in-place for known color keys. */
export const normalizeRawDefColors = (obj: unknown, key = ''): unknown => {
  if (COLOR_KEYS.has(key) && typeof obj === 'string' && HEX_COLOR_RE.test(obj)) {
    return parseInt(obj.slice(1), 16);
  }
  if (Array.isArray(obj)) return obj.map(v => normalizeRawDefColors(v));
  if (obj !== null && typeof obj === 'object') {
    return Object.fromEntries(
      Object.entries(obj as Record<string, unknown>).map(([k, v]) => [k, normalizeRawDefColors(v, k)])
    );
  }
  return obj;
};

/** A reference to a behavior function resolved from the virtual:behaviors module. */
export const BehaviorSpecSchema = z.object({
  /** Name of the behavior function (resolved from shared or level-local behaviors). */
  fn: z.string(),
  /** Parameters passed to the behavior function. */
  params: z.record(z.string(), z.unknown()).optional(),
});
export type BehaviorSpec = z.infer<typeof BehaviorSpecSchema>;

/** Spawner config: the object acts as a template that periodically creates clones. */
export const SpawnerDefSchema = z.object({
  /** Seconds between spawns. */
  interval: z.number(),
  /** Seconds before the first spawn. Default: 0 */
  initialDelay: z.number().optional(),
  /** Behaviors attached to each spawned clone. */
  behaviors: z.array(BehaviorSpecSchema).optional(),
});
export type SpawnerDef = z.infer<typeof SpawnerDefSchema>;

export const ParkourObjectMetaSchema = z.object({
  /** Checkpoint index (0, 1, 2…). Makes this object a mid-level respawn checkpoint. */
  checkpoint: z.number().optional(),
  /** If true, reaching this object ends the run. */
  win: z.boolean().optional(),
  /** Per-object boost-surface config; overrides any material-level config on the same object. */
  boostSurface: BoostSurfaceConfigSchema.optional(),
});
export type ParkourObjectMeta = z.infer<typeof ParkourObjectMetaSchema>;

export const ObjectDefSchema = z
  .object({
    id: z.string(),
    /** Key into the top-level `assets` registry */
    asset: z.string(),
    /** World-space position. Default: [0, 0, 0] */
    position: Vec3.optional(),
    /** Euler rotation in radians, YXZ order. Default: [0, 0, 0] */
    rotation: Vec3.optional(),
    /** Per-axis scale. Default: [1, 1, 1] */
    scale: Vec3.optional(),
    /** Default: true */
    castShadow: z.boolean().optional(),
    /** Default: true */
    receiveShadow: z.boolean().optional(),
    /** Passed through to object.userData; also checked for flags like `nocollide` */
    userData: z.record(z.string(), z.unknown()).optional(),
    /** Key into the top-level `materials` registry. All meshes in this object get this material. */
    material: z.string().optional(),
    /** If true, this object will not be registered in the collision world */
    nocollide: z.boolean().optional(),
    /**
     * Overrides the material-level `nonPermeable` flag for this object.
     * When true, the camera hard-snaps rather than dithering through it.
     * When false, disables non-permeable behavior even if the material sets it.
     * When absent, the material's own flag is used.
     */
    nonPermeable: z.boolean().optional(),
    /** Parkour-specific metadata: marks this object as a checkpoint or win zone. */
    parkour: ParkourObjectMetaSchema.optional(),
    ...ExternalVelocityDampingOverrideFields,
    /** Behaviors attached to this entity at load time.  Mutually exclusive with `spawner`. */
    behaviors: z.array(BehaviorSpecSchema).optional(),
    /** Spawner config: this object becomes a template that periodically creates clones.  Mutually exclusive with `behaviors`. */
    spawner: SpawnerDefSchema.optional(),
  })
  .refine(def => !(def.behaviors && def.spawner), {
    message: '"behaviors" and "spawner" are mutually exclusive on an object',
    path: ['spawner'],
  });

export type ObjectDef = z.infer<typeof ObjectDefSchema>;

export interface ObjectGroupDef {
  id: string;
  /**
   * Name of a generator (key into the top-level `generators` record) whose output populates
   * this group's children at load time. The group's own transform is fully editable.
   */
  generator?: string;
  children: (ObjectDef | ObjectGroupDef)[];
  position?: [number, number, number];
  rotation?: [number, number, number];
  scale?: [number, number, number];
  /** Runtime/editor metadata; generator output uses this to mark generated nodes as read-only. */
  userData?: Record<string, unknown>;
}

export const ObjectGroupDefSchema: z.ZodType<ObjectGroupDef> = z.lazy(() =>
  z.object({
    id: z.string(),
    generator: z.string().optional(),
    children: z.array(z.union([ObjectDefSchema, ObjectGroupDefSchema])),
    position: Vec3.optional(),
    rotation: Vec3.optional(),
    scale: Vec3.optional(),
    userData: z.record(z.string(), z.unknown()).optional(),
  })
);

export const ScenePhysicsDefSchema = z.object({
  gravity: z.number().optional(),
  simulationTickRate: z.number().optional(),
  gravityShaping: z
    .object({
      riseMultiplier: z.number().optional(),
      apexMultiplier: z.number().optional(),
      fallMultiplier: z.number().optional(),
      apexThreshold: z.number().optional(),
      kneeWidth: z.number().optional(),
      onlyJumps: z.boolean().optional(),
    })
    .optional(),
  player: z
    .object({
      jumpVelocity: z.number().optional(),
      moveSpeed: z.object({ onGround: z.number(), inAir: z.number() }).optional(),
      terminalVelocity: z.number().optional(),
      externalVelocityAirDampingFactor: Vec3.optional(),
    })
    .optional(),
});
export type ScenePhysicsDef = z.infer<typeof ScenePhysicsDefSchema>;

export const AmbientLightDefSchema = z.object({
  id: z.string(),
  type: z.literal('ambient'),
  /** Hex color integer. Default: 0xffffff */
  color: z.number().int().optional(),
  /** Default: 1 */
  intensity: z.number().optional(),
});

/**
 * Shadow map resolution. May be a single number (applied uniformly) or a per-quality object
 * so a single scene def can target Low/Medium/High tiers without scene code having to mutate
 * the light after instantiation.
 */
export const ShadowMapSizeSchema = z.union([
  z.number().int().positive(),
  z.object({
    low: z.number().int().positive(),
    medium: z.number().int().positive(),
    high: z.number().int().positive(),
  }),
]);
export type ShadowMapSize = z.infer<typeof ShadowMapSizeSchema>;

/**
 * Static shadow config for shadow-casting lights. Applied at light instantiation time,
 * BEFORE the light is added to the scene — this matters because three.js lazily creates
 * `light.shadow.map` at the current `mapSize` on first render and will NOT recreate it on
 * a subsequent mapSize change, so any post-hoc mutation after the first render is a no-op.
 */
export const ShadowConfigDefSchema = z.object({
  /** Shadow map resolution. Default: 1024 */
  mapSize: ShadowMapSizeSchema.optional(),
  /** Shadow acne bias. Default: 0 (often needs to be a small negative number like -0.0001) */
  bias: z.number().optional(),
  /** Normal-offset bias. Default: 0 */
  normalBias: z.number().optional(),
  /** PCF/VSM blur radius. Default: 1 */
  radius: z.number().optional(),
  /** VSM blur sample count. Default: 8 */
  blurSamples: z.number().int().positive().optional(),
  /** Directional/spot-only: near plane of the shadow camera frustum. Default: 0.5 */
  near: z.number().optional(),
  /** Directional/spot-only: far plane of the shadow camera frustum. Default: 500 */
  far: z.number().optional(),
  /** Directional-only: left edge of the orthographic shadow camera frustum. Default: -5 */
  left: z.number().optional(),
  /** Directional-only: right edge of the orthographic shadow camera frustum. Default: 5 */
  right: z.number().optional(),
  /** Directional-only: top edge of the orthographic shadow camera frustum. Default: 5 */
  top: z.number().optional(),
  /** Directional-only: bottom edge of the orthographic shadow camera frustum. Default: -5 */
  bottom: z.number().optional(),
});
export type ShadowConfigDef = z.infer<typeof ShadowConfigDefSchema>;

export const DirectionalLightDefSchema = z.object({
  id: z.string(),
  type: z.literal('directional'),
  color: z.number().int().optional(),
  intensity: z.number().optional(),
  /** World-space position (light shines from here toward origin). Default: [0, 1, 0] */
  position: Vec3.optional(),
  /** World-space target position the light shines toward. Default: [0, 0, 0] */
  target: Vec3.optional(),
  castShadow: z.boolean().optional(),
  /** Shadow camera + shadow map config. Only applied when `castShadow` is true. */
  shadow: ShadowConfigDefSchema.optional(),
});

export const PointLightDefSchema = z.object({
  id: z.string(),
  type: z.literal('point'),
  color: z.number().int().optional(),
  intensity: z.number().optional(),
  position: Vec3.optional(),
  /** Maximum range; 0 = unlimited. Default: 0 */
  distance: z.number().optional(),
  /** Attenuation exponent. Default: 2 */
  decay: z.number().optional(),
  castShadow: z.boolean().optional(),
  /** Shadow map config. Only applied when `castShadow` is true. Frustum fields are ignored
   * (point lights use a cube camera derived from `distance`). */
  shadow: ShadowConfigDefSchema.optional(),
});

export const SpotLightDefSchema = z.object({
  id: z.string(),
  type: z.literal('spot'),
  color: z.number().int().optional(),
  intensity: z.number().optional(),
  position: Vec3.optional(),
  /** World-space target position the spot light points toward. Default: [0, 0, 0] */
  target: Vec3.optional(),
  /** Half-angle of the cone in radians. Default: π/4 */
  angle: z.number().optional(),
  /** Edge softness 0–1. Default: 0 */
  penumbra: z.number().optional(),
  distance: z.number().optional(),
  decay: z.number().optional(),
  castShadow: z.boolean().optional(),
  /** Shadow camera + shadow map config. Only applied when `castShadow` is true.
   * Orthographic-frustum fields (left/right/top/bottom) are ignored (spot lights use a
   * perspective shadow camera derived from `angle`). */
  shadow: ShadowConfigDefSchema.optional(),
});

export const HemisphereLightDefSchema = z.object({
  id: z.string(),
  type: z.literal('hemisphere'),
  skyColor: z.number().int().optional(),
  groundColor: z.number().int().optional(),
  intensity: z.number().optional(),
});

export const RectAreaLightDefSchema = z.object({
  id: z.string(),
  type: z.literal('rectArea'),
  color: z.number().int().optional(),
  intensity: z.number().optional(),
  position: Vec3.optional(),
  /** Point the light faces; emits from its local -Z toward here. Default: [0, 0, 0] */
  target: Vec3.optional(),
  width: z.number().optional(),
  height: z.number().optional(),
  // three.js rect-area lights cast no shadow.
});

export const LightDefSchema = z.discriminatedUnion('type', [
  AmbientLightDefSchema,
  DirectionalLightDefSchema,
  PointLightDefSchema,
  SpotLightDefSchema,
  HemisphereLightDefSchema,
  RectAreaLightDefSchema,
]);

export type AmbientLightDef = z.infer<typeof AmbientLightDefSchema>;
export type DirectionalLightDef = z.infer<typeof DirectionalLightDefSchema>;
export type PointLightDef = z.infer<typeof PointLightDefSchema>;
export type SpotLightDef = z.infer<typeof SpotLightDefSchema>;
export type HemisphereLightDef = z.infer<typeof HemisphereLightDefSchema>;
export type RectAreaLightDef = z.infer<typeof RectAreaLightDefSchema>;
export type LightDef = z.infer<typeof LightDefSchema>;

// ---- Raw (disk-facing) light variants that accept hex color strings ----

const AmbientLightDefRawSchema = AmbientLightDefSchema.extend({ color: ColorInputSchema.optional() });
const DirectionalLightDefRawSchema = DirectionalLightDefSchema.extend({ color: ColorInputSchema.optional() });
const PointLightDefRawSchema = PointLightDefSchema.extend({ color: ColorInputSchema.optional() });
const SpotLightDefRawSchema = SpotLightDefSchema.extend({ color: ColorInputSchema.optional() });
const HemisphereLightDefRawSchema = HemisphereLightDefSchema.extend({
  skyColor: ColorInputSchema.optional(),
  groundColor: ColorInputSchema.optional(),
});
const RectAreaLightDefRawSchema = RectAreaLightDefSchema.extend({ color: ColorInputSchema.optional() });

const LightDefRawSchema = z.discriminatedUnion('type', [
  AmbientLightDefRawSchema,
  DirectionalLightDefRawSchema,
  PointLightDefRawSchema,
  SpotLightDefRawSchema,
  HemisphereLightDefRawSchema,
  RectAreaLightDefRawSchema,
]);

const GeneratorDefSchema = z.object({
  file: z.string(),
  params: z.record(z.string(), z.unknown()).optional(),
});

const GeneratorsRecordSchema = z.record(z.string(), GeneratorDefSchema);

/**
 * A named sample definition: an entry the runtime registers with the SFX
 * manager so subsequent scene-def references can play it by name.  Mirrors the
 * `SfxDef` type used by the runtime sound engine.
 */
export const SfxDefSchema = z.object({
  /** URL of the audio sample (.ogg/.wav/etc). */
  url: z.string(),
  /** Default playback rate when this sample is played. Default: 1 */
  playbackRate: z.number().optional(),
});
export type SfxDef = z.infer<typeof SfxDefSchema>;

const SpatialLoopFilterSchema = z.object({
  type: z.enum(['lp', 'hp', 'bp', 'notch']),
  freq: z.number(),
  q: z.number().optional(),
});

/**
 * A spatial looped sound source placed in the world.  Plays automatically when
 * the level loads.  The `sfx` field references either a builtin sfx name or
 * one of the keys in the level def's `audio.sfxDefs` map.
 */
export const SpatialLoopDefSchema = z.object({
  id: z.string(),
  /** Name of the sfx sample to loop (key into `sfxDefs` or a builtin). */
  sfx: z.string(),
  /** World-space position of the emitter. */
  pos: Vec3,
  /** Linear gain. Default: 1 */
  gain: z.number().optional(),
  /** Playback rate. Default: 1 */
  playbackRate: z.number().optional(),
  /** 0..1 fraction of the sample for crossfade ramp on each end. Default: 0.1 */
  xfade: z.number().optional(),
  /** Optional biquad filter applied to this voice. */
  filter: SpatialLoopFilterSchema.optional(),
  /** Distance at which attenuation begins. Default: 1 */
  refDistance: z.number().optional(),
  /** Attenuation curve exponent: gain = 1 / max(1, dist/ref)^rolloff. Default: 1 */
  rolloff: z.number().optional(),
  /** Linear gain threshold below which mixing is skipped. Default: 0.001 */
  cullThreshold: z.number().optional(),
});
export type SpatialLoopDef = z.infer<typeof SpatialLoopDefSchema>;

/**
 * Audio configuration for a level: scene-specific sfx sample registrations and
 * positioned spatial loops that start automatically at load time.  Lives at
 * `levelDef.audio` and may be sourced from the optional sidecar `audio.json`.
 */
export const AudioDefSchema = z.object({
  /** Map of sfx sample names → defs. Names override any builtin with the same key. */
  sfxDefs: z.record(z.string(), SfxDefSchema).optional(),
  /** Spatial loops started automatically when the level loads. */
  spatialLoops: z.array(SpatialLoopDefSchema).optional(),
});
export type AudioDef = z.infer<typeof AudioDefSchema>;

/** Collect all leaf ObjectDefs from a (possibly nested) objects array, with their schema paths. */
const collectObjectDefs = (
  nodes: (ObjectDef | ObjectGroupDef)[],
  prefix: (string | number)[]
): { obj: ObjectDef; path: (string | number)[] }[] => {
  const result: { obj: ObjectDef; path: (string | number)[] }[] = [];
  for (let i = 0; i < nodes.length; i++) {
    const node = nodes[i];
    if ('children' in node) {
      result.push(...collectObjectDefs(node.children, [...prefix, i, 'children']));
    } else {
      result.push({ obj: node, path: [...prefix, i] });
    }
  }
  return result;
};

export const GradientEnvironmentDefSchema = z.object({
  kind: z.literal('gradient'),
  skyColor: z.number().int().optional(),
  horizonColor: z.number().int().optional(),
  groundColor: z.number().int().optional(),
  intensity: z.number().optional(),
  setBackground: z.boolean().optional(),
});

export const EquirectEnvironmentDefSchema = z.object({
  kind: z.literal('equirect'),
  url: z.string(),
  intensity: z.number().optional(),
  setBackground: z.boolean().optional(),
});

export const EnvironmentDefSchema = z.discriminatedUnion('kind', [
  GradientEnvironmentDefSchema,
  EquirectEnvironmentDefSchema,
]);
export type EnvironmentDef = z.infer<typeof EnvironmentDefSchema>;

const GradientEnvironmentDefRawSchema = GradientEnvironmentDefSchema.extend({
  skyColor: ColorInputSchema.optional(),
  horizonColor: ColorInputSchema.optional(),
  groundColor: ColorInputSchema.optional(),
});
const EnvironmentDefRawSchema = z.discriminatedUnion('kind', [
  GradientEnvironmentDefRawSchema,
  EquirectEnvironmentDefSchema,
]);

export const LevelDefSchema = z
  .object({
    $schema: z.string().optional(),
    version: z.literal(1),
    /** Named texture definitions loaded in parallel at startup. */
    textures: z.record(z.string(), TextureDefSchema).optional(),
    /** Named material definitions built once their textures are ready. */
    materials: z.record(z.string(), MaterialDefSchema).optional(),
    assets: z.record(z.string(), AssetDefSchema),
    objects: z.array(z.union([ObjectDefSchema, ObjectGroupDefSchema])),
    lights: z.array(LightDefSchema).optional(),
    /** Scene-wide image-based lighting (IBL) + optional matching background. */
    environment: EnvironmentDefSchema.optional(),
    physics: ScenePhysicsDefSchema.optional(),
    generators: GeneratorsRecordSchema.optional(),
    audio: AudioDefSchema.optional(),
  })
  .superRefine((def, ctx) => {
    const assetKeys = new Set(Object.keys(def.assets));
    const matKeys = new Set(Object.keys(def.materials ?? {}));
    const texKeys = new Set(Object.keys(def.textures ?? {}));

    // Each object's asset and material must reference existing registry entries
    const allObjectDefs = collectObjectDefs(def.objects, ['objects']);
    for (const { obj, path } of allObjectDefs) {
      if (!assetKeys.has(obj.asset)) {
        ctx.addIssue({
          code: 'custom',
          path: [...path, 'asset'],
          message: `Unknown asset "${obj.asset}". Available: ${[...assetKeys].join(', ') || '(none)'}`,
        });
      }
      if (obj.material !== undefined && !matKeys.has(obj.material)) {
        ctx.addIssue({
          code: 'custom',
          path: [...path, 'material'],
          message: `Unknown material "${obj.material}". Available: ${[...matKeys].join(', ') || '(none)'}`,
        });
      }
    }

    // Validate CSG asset tree references
    for (const [assetName, assetDef] of Object.entries(def.assets)) {
      if (assetDef.type !== 'csg') continue;

      const validateNode = (node: CsgTreeNode, path: (string | number)[]) => {
        if ('asset' in node) {
          if (!assetKeys.has(node.asset)) {
            ctx.addIssue({
              code: 'custom',
              path: ['assets', assetName, ...path, 'asset'],
              message: `CSG leaf references unknown asset "${node.asset}"`,
            });
          }
          const refDef = def.assets[node.asset];
          if (refDef && refDef.type !== 'geoscript' && refDef.type !== 'csg') {
            ctx.addIssue({
              code: 'custom',
              path: ['assets', assetName, ...path, 'asset'],
              message: `CSG leaf must reference a geoscript or csg asset, got "${refDef.type}"`,
            });
          }
        } else {
          for (let i = 0; i < node.children.length; i++) {
            validateNode(node.children[i], [...path, 'children', i]);
          }
        }
      };

      validateNode(assetDef.tree, ['tree']);
    }

    // Each material's texture refs must reference existing texture entries
    for (const [matName, matDef] of Object.entries(def.materials ?? {})) {
      if (matDef.type !== 'customShader' || !matDef.props) continue;
      const p = matDef.props;
      for (const slot of TEXTURE_SLOTS) {
        const ref = p[slot];
        if (ref !== undefined && !texKeys.has(ref)) {
          ctx.addIssue({
            code: 'custom',
            path: ['materials', matName, 'props', slot],
            message: `Unknown texture "${ref}". Available: ${[...texKeys].join(', ') || '(none)'}`,
          });
        }
      }
    }
  });

export type LevelDef = z.infer<typeof LevelDefSchema>;

/**
 * Schema for level def JSON files on disk.
 * Geoscript assets may use either `code` (inline) or `file` (path relative to level dir).
 * Server-side loading inlines file refs and then validates against LevelDefSchema.
 */
export const LevelDefRawSchema = z.object({
  $schema: z.string().optional(),
  version: z.literal(1),
  textures: z.record(z.string(), TextureDefSchema).optional(),
  materials: z.record(z.string(), MaterialDefRawSchema).optional(),
  assets: z.record(z.string(), AssetDefRawSchema),
  objects: z.array(z.union([ObjectDefSchema, ObjectGroupDefSchema])),
  lights: z.array(LightDefRawSchema).optional(),
  environment: EnvironmentDefRawSchema.optional(),
  physics: ScenePhysicsDefSchema.optional(),
  generators: GeneratorsRecordSchema.optional(),
  audio: AudioDefSchema.optional(),
});

export type LevelDefRaw = z.infer<typeof LevelDefRawSchema>;

export const MaterialsFileSchema = z.object({
  $schema: z.string().optional(),
  textures: z.record(z.string(), TextureDefSchema).optional(),
  materials: z.record(z.string(), MaterialDefRawSchema).optional(),
});
export type MaterialsFile = z.infer<typeof MaterialsFileSchema>;

export const ObjectsFileSchema = z.object({
  $schema: z.string().optional(),
  objects: z.array(z.union([ObjectDefSchema, ObjectGroupDefSchema])),
});
export type ObjectsFile = z.infer<typeof ObjectsFileSchema>;

export const AudioFileSchema = z.object({
  $schema: z.string().optional(),
  sfxDefs: z.record(z.string(), SfxDefSchema).optional(),
  spatialLoops: z.array(SpatialLoopDefSchema).optional(),
});
export type AudioFile = z.infer<typeof AudioFileSchema>;

const BookmarkSlotSchema = z.number().int().min(0).max(9);

export const PlayBookmarkSchema = z.object({
  slot: BookmarkSlotSchema,
  mode: z.literal('play'),
  playerPos: Vec3,
  cameraAngles: z.object({ phi: z.number(), theta: z.number() }),
});

export const EditBookmarkSchema = z.object({
  slot: BookmarkSlotSchema,
  mode: z.literal('edit'),
  cameraPos: Vec3,
  orbitTarget: Vec3,
});

export const EditorBookmarkSchema = z.discriminatedUnion('mode', [PlayBookmarkSchema, EditBookmarkSchema]);
export type EditorBookmark = z.infer<typeof EditorBookmarkSchema>;

export const LocationsFileSchema = z.object({
  $schema: z.string().optional(),
  editor_bookmarks: z.array(EditorBookmarkSchema).optional(),
});
export type LocationsFile = z.infer<typeof LocationsFileSchema>;
