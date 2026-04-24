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
  /** @see Layer.oversample */
  oversample?: boolean;
}

export const voxelGroundLayer = (c: VoxelGroundLayerConfig): Layer => {
  const id = c.id;
  const defines: DefineContribution[] = [
    { key: 'MAX_VOX_DDA_STEPS', value: c.maxSteps ?? 48, merge: 'max' },
    { key: 'VOX_LAVA_QUALITY', value: c.lavaQuality ?? 0, merge: 'max' },
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
