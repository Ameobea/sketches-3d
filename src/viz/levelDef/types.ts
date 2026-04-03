import { z } from 'zod';

const Vec2 = z.tuple([z.number(), z.number()]);
const Vec3 = z.tuple([z.number(), z.number(), z.number()]);

export const GltfAssetDefSchema = z.object({
  type: z.literal('gltf'),
  /** Name of the mesh/object as it appears in the loaded gltf scene */
  meshName: z.string(),
});

export const GeoscriptAssetDefSchema = z.object({
  type: z.literal('geoscript'),
  code: z.string(),
  /** Whether to include the standard geoscript prelude. Default: true */
  includePrelude: z.boolean().optional(),
});

/** Geoscript asset that references an external file (resolved server-side before the client sees it). */
export const GeoscriptAssetDefFileSchema = z.object({
  type: z.literal('geoscript'),
  /** Path to a .geo file, relative to the level's directory. */
  file: z.string(),
  /** Whether to include the standard geoscript prelude. Default: true */
  includePrelude: z.boolean().optional(),
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
  /** Default: '' (NoColorSpace) */
  colorSpace: z.enum(['srgb', '']).optional(),
});

export type TextureDef = z.infer<typeof TextureDefSchema>;

export const AmbientDistanceAmpParamsSchema = z.object({
  falloffStartDistance: z.number(),
  falloffEndDistance: z.number(),
  exponent: z.number().optional(),
  ampFactor: z.number(),
});

export const ReflectionParamsSchema = z.object({
  alpha: z.number().optional(),
});

export const TriplanarMappingParamsJsonSchema = z.object({
  contrastPreservationFactor: z.number().optional(),
  sharpenFactor: z.number().optional(),
});

/** Serializable CustomShaderProps — texture slots as string keys into textures registry */
export const ShaderPropsJsonSchema = z.object({
  color: z.number().optional(),
  roughness: z.number().optional(),
  metalness: z.number().optional(),
  normalScale: z.number().optional(),
  emissiveIntensity: z.number().optional(),
  lightMapIntensity: z.number().optional(),
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
  // complex but JSON-safe
  ambientDistanceAmp: AmbientDistanceAmpParamsSchema.optional(),
  reflection: ReflectionParamsSchema.optional(),
});

/** Serializable subset of CustomShaderOptions */
export const ShaderOptionsJsonSchema = z.object({
  useTriplanarMapping: z.union([z.boolean(), TriplanarMappingParamsJsonSchema]).optional(),
  useGeneratedUVs: z.boolean().optional(),
  tileBreaking: z
    .union([
      z.object({ type: z.literal('neyret'), patchScale: z.number().optional() }),
      z.object({ type: z.literal('fastFixMipmap') }),
    ])
    .optional(),
  enableFog: z.boolean().optional(),
  antialiasColorShader: z.boolean().optional(),
  antialiasRoughnessShader: z.boolean().optional(),
  readRoughnessMapFromRChannel: z.boolean().optional(),
  disableToneMapping: z.boolean().optional(),
  randomizeUVOffset: z.boolean().optional(),
  useNoise2: z.boolean().optional(),
  materialClass: z.enum(['default', 'rock', 'crystal', 'instakill']).optional(),
});

export const CustomShaderMatDefSchema = z.object({
  type: z.literal('customShader'),
  props: ShaderPropsJsonSchema.optional(),
  options: ShaderOptionsJsonSchema.optional(),
  emissiveBypass: z.boolean().optional(),
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
});

/**
 * A material whose `THREE.Material` instance is provided at runtime via `LevelLoadHandle.setMaterialFactories`.
 * Used for fully custom materials that can't be described in data (e.g. animated or shader-heavy materials).
 */
export const GeneratedMatDefSchema = z.object({
  type: z.literal('generated'),
  emissiveBypass: z.boolean().optional(),
});

export const MaterialDefSchema = z.discriminatedUnion('type', [
  CustomShaderMatDefSchema,
  CustomBasicShaderMatDefSchema,
  GeneratedMatDefSchema,
]);

export type AmbientDistanceAmpParams = z.infer<typeof AmbientDistanceAmpParamsSchema>;
export type ReflectionParams = z.infer<typeof ReflectionParamsSchema>;
export type TriplanarMappingParamsJson = z.infer<typeof TriplanarMappingParamsJsonSchema>;
export type ShaderPropsJson = z.infer<typeof ShaderPropsJsonSchema>;
export type ShaderOptionsJson = z.infer<typeof ShaderOptionsJsonSchema>;
export type CustomShaderMatDef = z.infer<typeof CustomShaderMatDefSchema>;
export type CustomBasicShaderMatDef = z.infer<typeof CustomBasicShaderMatDefSchema>;
export type GeneratedMatDef = z.infer<typeof GeneratedMatDefSchema>;
export type MaterialDef = z.infer<typeof MaterialDefSchema>;

export const ParkourObjectMetaSchema = z.object({
  /** Checkpoint index (0, 1, 2…). Makes this object a mid-level respawn checkpoint. */
  checkpoint: z.number().optional(),
  /** If true, reaching this object ends the run. */
  win: z.boolean().optional(),
});
export type ParkourObjectMeta = z.infer<typeof ParkourObjectMetaSchema>;

export const ObjectDefSchema = z.object({
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
  /** Parkour-specific metadata: marks this object as a checkpoint or win zone. */
  parkour: ParkourObjectMetaSchema.optional(),
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

const GeneratorDefSchema = z.object({
  file: z.string(),
  params: z.record(z.string(), z.unknown()).optional(),
});

const GeneratorsRecordSchema = z.record(z.string(), GeneratorDefSchema);

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
    physics: ScenePhysicsDefSchema.optional(),
    generators: GeneratorsRecordSchema.optional(),
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
          code: z.ZodIssueCode.custom,
          path: [...path, 'asset'],
          message: `Unknown asset "${obj.asset}". Available: ${[...assetKeys].join(', ') || '(none)'}`,
        });
      }
      if (obj.material !== undefined && !matKeys.has(obj.material)) {
        ctx.addIssue({
          code: z.ZodIssueCode.custom,
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
              code: z.ZodIssueCode.custom,
              path: ['assets', assetName, ...path, 'asset'],
              message: `CSG leaf references unknown asset "${node.asset}"`,
            });
          }
          const refDef = def.assets[node.asset];
          if (refDef && refDef.type !== 'geoscript' && refDef.type !== 'csg') {
            ctx.addIssue({
              code: z.ZodIssueCode.custom,
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
      const texSlots = [
        'map',
        'normalMap',
        'roughnessMap',
        'metalnessMap',
        'lightMap',
        'transmissionMap',
        'clearcoatNormalMap',
      ] as const;
      for (const slot of texSlots) {
        const ref = p[slot];
        if (ref !== undefined && !texKeys.has(ref)) {
          ctx.addIssue({
            code: z.ZodIssueCode.custom,
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
  materials: z.record(z.string(), MaterialDefSchema).optional(),
  assets: z.record(z.string(), AssetDefRawSchema),
  objects: z.array(z.union([ObjectDefSchema, ObjectGroupDefSchema])),
  physics: ScenePhysicsDefSchema.optional(),
  generators: GeneratorsRecordSchema.optional(),
});

export type LevelDefRaw = z.infer<typeof LevelDefRawSchema>;

/**
 * Schema for an optional `materials.json` file alongside `def.json`.
 * When present, its textures and materials are merged over (and replace) any
 * same-named entries in `def.json`, allowing the materials layer to live in a
 * separate file and keeping `def.json` focused on assets and objects.
 */
export const MaterialsFileSchema = z.object({
  $schema: z.string().optional(),
  textures: z.record(z.string(), TextureDefSchema).optional(),
  materials: z.record(z.string(), MaterialDefSchema).optional(),
});
export type MaterialsFile = z.infer<typeof MaterialsFileSchema>;

/**
 * Schema for an optional `objects.json` file alongside `def.json`.
 * When present, its `objects` array is used in place of the one in `def.json`,
 * so the high-churn placement data can be diffed separately from the rest of
 * the level definition.
 */
export const ObjectsFileSchema = z.object({
  $schema: z.string().optional(),
  objects: z.array(z.union([ObjectDefSchema, ObjectGroupDefSchema])),
});
export type ObjectsFile = z.infer<typeof ObjectsFileSchema>;
