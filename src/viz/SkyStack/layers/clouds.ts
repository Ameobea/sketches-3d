import * as THREE from 'three';

import type { Layer } from '../types';
import { resolveId } from './_util';
import cloudsInstanceGlsl from './clouds.instance.glsl?raw';
import cloudsModuleGlsl from './clouds.module.glsl?raw';

export interface CloudsLayerConfig {
  id: string;
  zIndex: number;
  /** Cloud color for thin / low-density regions (wispy edges). */
  color: THREE.ColorRepresentation;
  /** Cloud color for dense / high-density cores. Defaults to `color`. */
  highColor?: THREE.ColorRepresentation;
  /** Peak opacity of the layer in [0, 1]. */
  intensity: number;
  /** Elevation center in [-1, 1] (after horizonOffset). */
  center: number;
  /** Half-width in elevation units. */
  width: number;
  /** Edge sharpness of the noise threshold, in (0, 0.5]. Default 0.15. */
  sharpness?: number;
  /** Anisotropic scale applied before sampling 3D noise. Default [1, 4, 1]. */
  scale?: [number, number, number];
  /** Per-axis drift speed (units/second). Default [0, 0, 0]. */
  speed?: [number, number, number];
  /** fBm octave count. Default 4. */
  octaves?: number;
  /** fBm frequency multiplier per octave. Default 2.0. */
  lacunarity?: number;
  /** fBm amplitude multiplier per octave. Default 0.5. */
  gain?: number;
  /** Density bias added before thresholding. Default 0. */
  bias?: number;
  /** Shaping exponent on the density curve. Default 1. */
  pow?: number;
}

const vec3Uniform = (v: [number, number, number] | undefined, fallback: [number, number, number]) =>
  new THREE.Vector3(...(v ?? fallback));

export const cloudsLayer = (c: CloudsLayerConfig): Layer => {
  const id = c.id;
  const octaves = c.octaves ?? 4;
  const uniforms: Record<string, THREE.IUniform> = {
    [`uHazeColor_${id}`]: { value: new THREE.Color(c.color) },
    [`uHazeHighColor_${id}`]: { value: new THREE.Color(c.highColor ?? c.color) },
    [`uHazeIntensity_${id}`]: { value: c.intensity },
    [`uHazeCenter_${id}`]: { value: c.center },
    [`uHazeWidth_${id}`]: { value: c.width },
    [`uHazeSharpness_${id}`]: { value: c.sharpness ?? 0.15 },
    [`uHazeScale_${id}`]: { value: vec3Uniform(c.scale, [1, 4, 1]) },
    [`uHazeSpeed_${id}`]: { value: vec3Uniform(c.speed, [0, 0, 0]) },
    [`uHazeOctaves_${id}`]: { value: octaves },
    [`uHazeLacunarity_${id}`]: { value: c.lacunarity ?? 2.0 },
    [`uHazeGain_${id}`]: { value: c.gain ?? 0.5 },
    [`uHazeBias_${id}`]: { value: c.bias ?? 0.0 },
    [`uHazePow_${id}`]: { value: c.pow ?? 1.0 },
  };

  return {
    id,
    zIndex: c.zIndex,
    uniforms,
    // skyFbm is shared across all cloud instances — dedup'd by key.
    modules: [{ key: 'clouds.skyFbm', glsl: cloudsModuleGlsl }],
    // Every cloud instance contributes its octave count; the composer takes
    // the max across all instances to bake MAX_HAZE_OCTAVES as the loop bound.
    defines: [{ key: 'MAX_HAZE_OCTAVES', value: Math.max(octaves, 1), merge: 'max' }],
    instanceGlsl: resolveId(cloudsInstanceGlsl, id),
    body: resolveId(
      `vec4 haze = sampleHaze_$ID(dir, elev);
      // Alpha-blend cloud — color premultiplied by haze.a at the call site.
      accumulate(haze.rgb * haze.a, vec3(0.0), haze.a, 0.0);`,
      id
    ),
    gate: 'aboveHorizon',
  };
};
