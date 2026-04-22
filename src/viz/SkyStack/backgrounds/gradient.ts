import * as THREE from 'three';

import { resolveId } from '../layers/_util';
import type { BackgroundLayer } from '../types';
import gradientGlsl from './gradient.glsl?raw';
import { computeGradientLut } from './oklab';

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

export interface GradientBackgroundConfig {
  /** Defaults to 'background_gradient'. Override for multiple gradient-style backgrounds. */
  id?: string;
  stops: GradientStop[];
  /** Mode for below-horizon color. Default Mirror. */
  horizonMode?: HorizonMode;
  /** Used when horizonMode = SolidBelow. Default 0x000000. */
  belowColor?: THREE.ColorRepresentation;
  bands?: CloudBand[];
  /**
   * 1D LUT resolution. The gradient is Oklab-interpolated at factory time
   * into `lutResolution` evenly-spaced RGB entries, baked as a constant
   * array into the shader. The fragment shader does a cheap linear RGB
   * lerp between adjacent entries — perceptually indistinguishable from
   * live Oklab math for typical close-hue sky gradients, but without any
   * per-fragment cube roots.
   *
   * Higher = smoother gradient (more shader constants emitted), lower =
   * smaller shader + may show faint banding on long smooth ramps. 32 is
   * usually fine; 128 is overkill for sky. Default 64.
   */
  lutResolution?: number;
}

const emitLutConst = (id: string, lut: readonly (readonly [number, number, number])[]): string => {
  const entries = lut
    .map(([r, g, b]) => `vec3(${r.toFixed(6)}, ${g.toFixed(6)}, ${b.toFixed(6)})`)
    .join(',\n  ');
  return `const vec3 GRADIENT_LUT_${id}[${lut.length}] = vec3[](\n  ${entries}\n);`;
};

/**
 * Gradient-sky background. Oklab gradient is baked at factory time into a
 * constant 1D LUT of length `lutResolution`; the per-fragment lookup is an
 * index + linear RGB lerp between adjacent entries. Stops are effectively
 * frozen at construction — mutating them live isn't supported because the
 * LUT is a shader constant. Rebuild the SkyStack to change them.
 *
 * Optional additive bands stay as animatable uniforms (intensity, fade rate,
 * color per band) — they're overlayed on top of the LUT sample.
 */
export const gradientBackground = (c: GradientBackgroundConfig): BackgroundLayer => {
  if (c.stops.length < 1) {
    throw new Error('gradientBackground: must have at least one stop');
  }
  const id = c.id ?? 'background_gradient';
  const lutResolution = c.lutResolution ?? 64;
  if (lutResolution < 2) {
    throw new Error(`gradientBackground: lutResolution must be >= 2, got ${lutResolution}`);
  }

  const lut = computeGradientLut(c.stops, lutResolution);
  const lutConstGlsl = emitLutConst(id, lut);

  const bands = c.bands ?? [];
  const bandCount = bands.length;
  const bandCenters = new Float32Array(bandCount);
  const bandWidths = new Float32Array(bandCount);
  const bandIntensities = new Float32Array(bandCount);
  const bandFadeRates = new Float32Array(bandCount);
  const bandFadePhases = new Float32Array(bandCount);
  const bandColors: THREE.Color[] = [];
  for (let i = 0; i < bandCount; i++) {
    const b = bands[i];
    bandCenters[i] = b.center;
    bandWidths[i] = b.width;
    bandIntensities[i] = b.intensity;
    bandFadeRates[i] = b.fadeRate ?? 0;
    bandFadePhases[i] = b.fadePhase ?? 0;
    bandColors.push(new THREE.Color(b.color));
  }

  const uniforms: Record<string, THREE.IUniform> = {
    [`uHorizonMode_${id}`]: { value: c.horizonMode ?? HorizonMode.Mirror },
    [`uBelowColor_${id}`]: { value: new THREE.Color(c.belowColor ?? 0x000000) },
  };
  if (bandCount > 0) {
    uniforms[`uBandCenters_${id}`] = { value: bandCenters };
    uniforms[`uBandWidths_${id}`] = { value: bandWidths };
    uniforms[`uBandColors_${id}`] = { value: bandColors };
    uniforms[`uBandIntensities_${id}`] = { value: bandIntensities };
    uniforms[`uBandFadeRates_${id}`] = { value: bandFadeRates };
    uniforms[`uBandFadePhases_${id}`] = { value: bandFadePhases };
  }

  return {
    id,
    uniforms,
    // Per-instance defines (unique key per id) — merge op is irrelevant for
    // a single contributor; 'max' is idempotent.
    defines: [
      { key: `LUT_SIZE_${id}`, value: lutResolution, merge: 'max' },
      { key: `BAND_COUNT_${id}`, value: bandCount, merge: 'max' },
    ],
    instanceGlsl: `${lutConstGlsl}\n${resolveId(gradientGlsl, id)}`,
    body: resolveId(
      `vec3 g = evalGradient_$ID(elev, horizonBlend) +
          evalBands_$ID(elev, cosElev) * horizonBlend;
      accumulate(g, vec3(0.0), 1.0, 0.0);`,
      id
    ),
  };
};
