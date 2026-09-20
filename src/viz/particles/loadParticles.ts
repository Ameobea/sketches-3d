import type * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality } from 'src/viz/conf';
import { resolveCustomUniforms } from 'src/viz/materials/buildMaterial';
import { invalidateHeightFields } from './HeightField';
import { ParticleSystem } from './ParticleSystem';
import type { ParticleSystemDef } from './schema';

const QUALITY_KEY = {
  [GraphicsQuality.Low]: 'low',
  [GraphicsQuality.Medium]: 'medium',
  [GraphicsQuality.High]: 'high',
} as const;
const DEFAULT_QUALITY_MUL = { low: 0, medium: 1, high: 1 };

export const loadLevelParticles = (
  viz: Viz,
  defs: ParticleSystemDef[],
  quality: GraphicsQuality,
  loadedTextures: ReadonlyMap<string, THREE.Texture>,
  texturesLoaded: Promise<void>
): void => {
  // maps rendered while level geometry was still streaming in are missing occluders
  void texturesLoaded.then(() => invalidateHeightFields(viz));
  for (const def of defs) {
    const key = QUALITY_KEY[quality];
    const count = Math.round(def.count * (def.quality?.[key] ?? DEFAULT_QUALITY_MUL[key]));
    if (count <= 0 || def.enabled === false) continue;
    const uniformsJson = def.shaders.customUniforms ?? {};
    const create = () => {
      const sys = new ParticleSystem(viz, def, count, resolveCustomUniforms(uniformsJson, loadedTextures));
      viz.particleSystems.push(sys);
      viz.registerDestroyedCb(() => sys.dispose());
    };
    if (Object.values(uniformsJson).some(u => u.type === 'sampler2D')) void texturesLoaded.then(create);
    else create();
  }
};
