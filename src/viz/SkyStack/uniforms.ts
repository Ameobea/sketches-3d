import * as THREE from 'three';

export const HorizonMode = {
  SolidBelow: 0,
  Mirror: 1,
  Extend: 2,
} as const;
export type HorizonMode = (typeof HorizonMode)[keyof typeof HorizonMode];

export interface GradientStop {
  /** 0 = horizon, 1 = zenith. Stops must be sorted ascending. */
  position: number;
  color: THREE.ColorRepresentation;
}

export interface CloudBand {
  /** Elevation center in [-1, 1]. 0 = horizon, 1 = zenith, -1 = nadir. */
  center: number;
  /** Falloff half-width in the same units as `center`. */
  width: number;
  color: THREE.ColorRepresentation;
  intensity: number;
  /** Fade rate in rad/s. 0 disables animation. */
  fadeRate?: number;
  fadePhase?: number;
}

/**
 * Uniforms shared by every layer in a SkyStack. The stop / band counts are
 * baked into the shader as `#define`s at construction time (loop bounds become
 * literal constants the driver can unroll), so the corresponding uniform
 * arrays are sized exactly to the configured count — no slack, no MAX cap.
 *
 * Mutating array contents at runtime is fine (e.g. `setStops` for a time-of-
 * day color shift), but the *count* is fixed at construction. Reconfiguring
 * with a different count requires recreating the SkyStack.
 */
export interface SkyStackUniforms {
  uTime: THREE.IUniform<number>;
  uHorizonOffset: THREE.IUniform<number>;
  uProjectionMatrixInverse: THREE.IUniform<THREE.Matrix4>;
  uCameraWorldMatrix: THREE.IUniform<THREE.Matrix4>;
  uSceneDepth: THREE.IUniform<THREE.Texture | null>;

  uStopPositions: THREE.IUniform<Float32Array>;
  uStopColors: THREE.IUniform<THREE.Color[]>;
  uHorizonMode: THREE.IUniform<number>;
  uBelowColor: THREE.IUniform<THREE.Color>;
  uHorizonBlend: THREE.IUniform<number>;

  uBandCenters: THREE.IUniform<Float32Array>;
  uBandWidths: THREE.IUniform<Float32Array>;
  uBandColors: THREE.IUniform<THREE.Color[]>;
  uBandIntensities: THREE.IUniform<Float32Array>;
  uBandFadeRates: THREE.IUniform<Float32Array>;
  uBandFadePhases: THREE.IUniform<Float32Array>;
}

const makeColorArray = (n: number): THREE.Color[] => {
  const out: THREE.Color[] = [];
  for (let i = 0; i < n; i++) {
    out.push(new THREE.Color());
  }
  return out;
};

export interface SkyStackUniformCounts {
  /** Number of gradient stops. Must be >= 1. */
  stopCount: number;
  /** Number of cloud bands. May be 0. */
  bandCount: number;
}

export const createSkyStackUniforms = ({ stopCount, bandCount }: SkyStackUniformCounts): SkyStackUniforms => {
  if (stopCount < 1) {
    throw new Error(`SkyStack requires at least one gradient stop, got ${stopCount}`);
  }
  return {
    uTime: { value: 0 },
    uHorizonOffset: { value: 0 },
    uProjectionMatrixInverse: { value: new THREE.Matrix4() },
    uCameraWorldMatrix: { value: new THREE.Matrix4() },
    uSceneDepth: { value: null },

    uStopPositions: { value: new Float32Array(stopCount) },
    uStopColors: { value: makeColorArray(stopCount) },
    uHorizonMode: { value: HorizonMode.Mirror },
    uBelowColor: { value: new THREE.Color() },
    uHorizonBlend: { value: 0 },

    uBandCenters: { value: new Float32Array(bandCount) },
    uBandWidths: { value: new Float32Array(bandCount) },
    uBandColors: { value: makeColorArray(bandCount) },
    uBandIntensities: { value: new Float32Array(bandCount) },
    uBandFadeRates: { value: new Float32Array(bandCount) },
    uBandFadePhases: { value: new Float32Array(bandCount) },
  };
};

/** Cast helper: three.js ShaderMaterial accepts any string-keyed uniform record. */
export const asUniformRecord = (u: SkyStackUniforms): Record<string, THREE.IUniform> =>
  u as unknown as Record<string, THREE.IUniform>;
