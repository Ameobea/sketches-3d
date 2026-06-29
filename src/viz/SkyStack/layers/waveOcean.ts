import type { DefineContribution, Layer } from '../types';
import { resolveId } from './_util';
import waveOceanGlsl from './waveOcean.glsl?raw';

export interface WaveOceanLayerConfig {
  id: string;
  zIndex: number;
  /** Max Lipschitz march steps before falling back to the last marched position. Default 96. */
  maxSteps?: number;
  /**
   * Multiplies the LoD distance footprint — higher fades detail nearer (cheaper).
   * Default 1.0; bump on lower quality tiers. Affects the swell/hump distance
   * fades, not the resolution-independent grazing fade.
   */
  lodBias?: number;
  /**
   * Debug render mode (default 0 = off).
   *   1 = march step-count heatmap (dim blue = cheap, red = full budget).
   */
  debugMode?: number;
  /** @see Layer.oversample */
  oversample?: boolean | 2 | 3 | 4;
}

export const waveOceanLayer = (c: WaveOceanLayerConfig): Layer => {
  const id = c.id;
  const defines: DefineContribution[] = [
    { key: `MAX_OCEAN_STEPS_${id}`, value: c.maxSteps ?? 96, merge: 'max' },
    { key: `OCEAN_LOD_BIAS_${id}`, value: c.lodBias ?? 1.0, merge: 'max' },
    { key: 'DEBUG_OCEAN_MODE', value: c.debugMode ?? 0, merge: 'max' },
  ];

  return {
    id,
    zIndex: c.zIndex,
    uniforms: {},
    defines,
    instanceGlsl: resolveId(waveOceanGlsl, id),
    body: resolveId(
      `vec3 woColor_$ID;
      float woAlpha_$ID;
      sampleWaveOcean_$ID(dir, woColor_$ID, woAlpha_$ID);
      accumulate(woColor_$ID, vec3(0.0), woAlpha_$ID, 0.0);`,
      id
    ),
    gate: 'dir.y < 0.01',
    oversample: c.oversample,
  };
};
