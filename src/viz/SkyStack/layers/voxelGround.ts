import type { DefineContribution, Layer } from '../types';
import { resolveId } from './_util';
import voxelGroundGlsl from './voxelGround.glsl?raw';

export interface VoxelGroundLayerConfig {
  id: string;
  zIndex: number;
  /** Max DDA steps before giving up. Default 48. */
  maxSteps?: number;
  /** Lava surface quality: 0 = simple, 1 = 3D fbm noise. Default 0. */
  lavaQuality?: number;
  /**
   * Debug render mode (default 0 = off).
   *   1 = DDA step-count heatmap. Dim blue = few steps, red = full step budget,
   *       very dim flat = LOD-skipped (zero DDA cost). Override of the normal
   *       coloring; emissive output is suppressed.
   *   2 = Cell-cache miss heatmap. Same ramp as mode 1 (red = many recomputes),
   *       but counts only DDA steps that had to redo the per-cell hash work
   *       instead of reusing the cached `CellData`. Compare with mode 1 — the
   *       gap between the two is the work the cache is saving. Should read
   *       substantially cooler than mode 1 wherever caching is helping.
   */
  debugMode?: number;
  /** @see Layer.oversample */
  oversample?: boolean | 2 | 3 | 4;
}

export const voxelGroundLayer = (c: VoxelGroundLayerConfig): Layer => {
  const id = c.id;
  const defines: DefineContribution[] = [
    { key: 'MAX_VOX_DDA_STEPS', value: c.maxSteps ?? 48, merge: 'max' },
    { key: 'VOX_LAVA_QUALITY', value: c.lavaQuality ?? 0, merge: 'max' },
    { key: 'DEBUG_VOX_GROUND_MODE', value: c.debugMode ?? 0, merge: 'max' },
  ];

  return {
    id,
    zIndex: c.zIndex,
    uniforms: {},
    defines,
    instanceGlsl: resolveId(voxelGroundGlsl, id),
    body: resolveId(
      `vec3 vgColor_$ID, vgEmissive_$ID;
      float vgAlpha_$ID, vgEmissiveAlpha_$ID;
      sampleVoxelGround_$ID(dir, vgColor_$ID, vgEmissive_$ID, vgAlpha_$ID, vgEmissiveAlpha_$ID);
      accumulate(vgColor_$ID, vgEmissive_$ID, vgAlpha_$ID, vgEmissiveAlpha_$ID);`,
      id
    ),
    gate: 'dir.y < 0.01',
    oversample: c.oversample,
  };
};
