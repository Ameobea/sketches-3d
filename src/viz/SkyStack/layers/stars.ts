import * as THREE from 'three';

import type { Layer } from '../types';
import { resolveId } from './_util';
import starsGlsl from './stars.glsl?raw';

export interface StarsLayerConfig {
  id: string;
  zIndex: number;
  color?: THREE.ColorRepresentation;
  /** Overall brightness multiplier. 0 disables the layer entirely. */
  intensity: number;
  /** Cells across one full azimuth turn at the horizon. 64–256 is reasonable. */
  density: number;
  /** Fraction of cells that actually contain a star, in [0, 1]. */
  threshold: number;
  /** Star point size in local cell units. 0.02–0.1 works well. */
  size: number;
  /** Fast twinkle rate in rad/s. 0 disables twinkle. */
  twinkleSpeed: number;
  /** Max brightness dip from twinkling, in [0, 1]. Default 0.25. */
  twinkleDepth?: number;
  /** Elevation at which stars are fully on. Default 0.04. */
  minElev?: number;
  /** Half-width of the fade ramp around `minElev`. Default 0.03. */
  fadeRange?: number;
  /** @see Layer.oversample */
  oversample?: boolean | 2 | 3 | 4;
}

export const starsLayer = (c: StarsLayerConfig): Layer => {
  const id = c.id;
  const uniforms: Record<string, THREE.IUniform> = {
    [`uStarColor_${id}`]: { value: new THREE.Color(c.color ?? 0xffffff) },
    [`uStarIntensity_${id}`]: { value: c.intensity },
    [`uStarDensity_${id}`]: { value: c.density },
    [`uStarThreshold_${id}`]: { value: c.threshold },
    [`uStarSize_${id}`]: { value: c.size },
    [`uStarTwinkleSpeed_${id}`]: { value: c.twinkleSpeed },
    [`uStarTwinkleDepth_${id}`]: { value: c.twinkleDepth ?? 0.25 },
    [`uStarMinElev_${id}`]: { value: c.minElev ?? 0.04 },
    [`uStarFadeRange_${id}`]: { value: c.fadeRange ?? 0.03 },
  };

  return {
    id,
    zIndex: c.zIndex,
    uniforms,
    instanceGlsl: resolveId(starsGlsl, id),
    body: resolveId(
      `vec4 stars = sampleStars_$ID(dir, elev, azimuth, cosElev);
      // Pure emissive: no skyColor contribution. Stars behind alpha-blend
      // layers auto-attenuate via the (1 - accumAlpha) weight in accumulate().
      accumulate(vec3(0.0), stars.rgb, 0.0, stars.a);`,
      id
    ),
    gate: 'aboveHorizon',
    oversample: c.oversample,
  };
};
