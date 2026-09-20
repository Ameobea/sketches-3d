import { z } from 'zod';

import { CustomUniformJsonSchema, ShaderConstantJsonSchema } from 'src/viz/materials/schema';

const Vec3 = z.tuple([z.number(), z.number(), z.number()]);

const gateAffects = {
  /** What the gate scales. `density` thins particles stochastically at constant size. Default: `alpha` */
  affects: z.enum(['alpha', 'size', 'density']).optional(),
  range: z.tuple([z.number(), z.number()]),
};

const DistanceGateSchema = z.object({
  /** `nearFade`: 0 → 1 across `range` of camera distance; `farFade`: 1 → 0. */
  gate: z.enum(['nearFade', 'farFade']),
  ...gateAffects,
});

const ExposureGateSchema = z.object({
  /**
   * 0 → 1 across `range` metres of clearance from the first static surface between the particle and
   * a horizontal plane: `skyExposed` looks down from `planeY` (rain dies under cover), `floorExposed`
   * looks up from it (dust rising out of a fog floor stops under platforms). Backed by a
   * camera-following height map that systems with identical settings share.
   */
  gate: z.enum(['skyExposed', 'floorExposed']),
  ...gateAffects,
  planeY: z.number(),
  /** How far past the plane the map sees. Default: 1024 */
  depth: z.number().positive().optional(),
  /** World size of the map; needs ~10% headroom over the volume's XZ size. Default: 1.25 × the larger of the two */
  extent: z.number().positive().optional(),
  /** Default: 1024 */
  resolution: z.number().int().positive().optional(),
  /** Per-particle lookup jitter in world units, dithering the map's texel-stepped edges. Default: 1 */
  soften: z.number().min(0).optional(),
});
export type ExposureGate = z.infer<typeof ExposureGateSchema>;

export const ParticleGateSchema = z.union([DistanceGateSchema, ExposureGateSchema]);
export type ParticleGate = z.infer<typeof ParticleGateSchema>;

/** `edgeFade` = fade width at the faces. Default: 10% of the smallest dimension */
const volumeCommon = { size: Vec3, edgeFade: z.number().min(0).optional() };

/**
 * The window onto the world-anchored, toroidally repeating particle field. Its faces fade, so any
 * axis that follows the camera breathes as the camera moves along it (a jump, for a short box).
 */
export const ParticleVolumeSchema = z.discriminatedUnion('kind', [
  z.object({
    kind: z.literal('cameraBox'),
    ...volumeCommon,
    /** Box centre relative to the camera. Exposure-gate maps stay camera-centred, so budget their `extent` for it. */
    offset: Vec3.optional(),
  }),
  z.object({
    /** Follows the camera in XZ only; Y is the fixed world band `centerY ± size.y / 2`. */
    kind: z.literal('cameraSlab'),
    ...volumeCommon,
    centerY: z.number(),
    /** XZ of the slab centre relative to the camera. */
    offset: z.tuple([z.number(), z.number()]).optional(),
    /**
     * Skews spawn origins along the band instead of spreading them evenly, so a tall band can stay
     * sparse where it matters less: `power` 1 is uniform, higher packs harder toward `toward`.
     * Shapes origins only; long vertical drifts carry particles back out of it.
     */
    distributionY: z
      .object({ toward: z.enum(['bottom', 'center', 'top']), power: z.number().positive() })
      .optional(),
  }),
]);
export type ParticleVolume = z.infer<typeof ParticleVolumeSchema>;

const shaderExtras = {
  customUniforms: z.record(z.string(), CustomUniformJsonSchema).optional(),
  constants: z.record(z.string(), ShaderConstantJsonSchema).optional(),
};

/**
 * A stateless GPU particle system. Every particle is a pure function of `(seed, age, spawnOrigin,
 * spawnParams)` evaluated in the vertex shader by the `motion` slot; `sprite` shades the quad.
 */
export const ParticleSystemDefSchema = z.object({
  id: z.string(),
  /** Default: true */
  enabled: z.boolean().optional(),
  /** Particle count at a quality multiplier of 1. */
  count: z.number().int().positive(),
  /** Count multipliers per graphics quality; 0 disables. Default: low 0, medium 1, high 1 */
  quality: z
    .object({ low: z.number().min(0), medium: z.number().min(0), high: z.number().min(0) })
    .partial()
    .optional(),
  volume: ParticleVolumeSchema,
  /** Seconds before a particle respawns at a new origin. Omit for immortal drifters. */
  lifetime: z.number().positive().optional(),
  /** Default: `billboard` */
  orient: z.enum(['billboard', 'velocityStretch']).optional(),
  /** `velocityStretch` only: seconds of travel the streak spans. Default: 0.05 */
  stretch: z.number().min(0).optional(),
  /** `scene`: lit, fogged, tone-mapped. `emissive`: bypass buffer, blooms, forces additive blend. Default: `scene` */
  output: z.enum(['scene', 'emissive']).optional(),
  /** `sunGated`: ambient + directional light, shadow-gated when the light casts. Default: `unlit` */
  light: z.enum(['unlit', 'sunGated']).optional(),
  /** Default: `premultiplied` */
  blend: z.enum(['premultiplied', 'additive']).optional(),
  /** Projected size clamp in pixels; below `min` the sprite grows and dims to preserve coverage. Default: min 1.5, max 128 */
  sizePx: z.object({ min: z.number().min(0).optional(), max: z.number().positive().optional() }).optional(),
  /**
   * World-unit depth over which sprites fade out ahead of the geometry behind them, for large soft
   * sprites that would otherwise clip against it in hard lines. Depth-tests per fragment in the
   * shader instead of in hardware, so leave it off for dense systems of tiny sprites.
   */
  softDepth: z.number().positive().optional(),
  gates: z.array(ParticleGateSchema).optional(),
  shaders: z.object({
    /** `Particle motion(float seed, float age, vec3 origin, vec4 params)` — see particle.vert for the struct. */
    motion: z.string(),
    /** `vec4 sprite(vec2 uv, float seed, float age)` — straight (non-premultiplied) rgba. */
    sprite: z.string(),
    ...shaderExtras,
  }),
});
export type ParticleSystemDef = z.infer<typeof ParticleSystemDefSchema>;

const GlslFieldRaw = z.union([z.string(), z.object({ file: z.string() })]);

export const ParticleSystemDefRawSchema = ParticleSystemDefSchema.extend({
  shaders: z.object({ motion: GlslFieldRaw, sprite: GlslFieldRaw, ...shaderExtras }),
});
export type ParticleSystemDefRaw = z.infer<typeof ParticleSystemDefRawSchema>;

export const PARTICLE_GLSL_FIELDS = ['motion', 'sprite'] as const;
