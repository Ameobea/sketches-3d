import type * as THREE from 'three';

export interface CloudsLayerConfig {
  /** Cloud color for thin / low-density regions (wispy edges). */
  color: THREE.ColorRepresentation;
  /**
   * Cloud color for dense / high-density cores. Oklab-mixed with `color` by
   * the shaped fBm value. Defaults to `color` (single-hue cloud).
   */
  highColor?: THREE.ColorRepresentation;
  /** Peak opacity of the layer in [0, 1]. */
  intensity: number;
  /** Elevation center in [-1, 1] (after the shared horizonOffset is applied). */
  center: number;
  /** Half-width in elevation units. */
  width: number;
  /**
   * Edge sharpness of the noise threshold, in (0, 0.5]. Smaller = crisper,
   * wispier features; larger = softer, more diffuse. Default 0.15.
   */
  sharpness?: number;
  /**
   * Anisotropic scale applied to the direction vector before sampling 3D
   * noise. Large y with small x/z produces horizontal streaking; uniform
   * values give isotropic puffs. Default [1, 4, 1].
   */
  scale?: [number, number, number];
  /** Per-axis drift speed (units/second). Default [0, 0, 0] (static). */
  speed?: [number, number, number];
  /** fBm octave count, capped at 6. Default 4. */
  octaves?: number;
  /** fBm frequency multiplier per octave. Default 2.0. */
  lacunarity?: number;
  /** fBm amplitude multiplier per octave. Default 0.5. */
  gain?: number;
  /**
   * Offset added to fBm output before thresholding. Positive = denser
   * coverage, negative = sparser. Typical [-0.3, 0.3]. Default 0.
   */
  bias?: number;
  /**
   * Exponent on the (biased) fBm output. >1 crushes lows (crisper towers),
   * <1 lifts them (softer haze). Default 1.
   */
  pow?: number;
}
