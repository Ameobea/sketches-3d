import type * as THREE from 'three';

export interface StarsLayerConfig {
  /** Star color tint. Default 0xffffff. */
  color?: THREE.ColorRepresentation;
  /** Overall brightness multiplier. 0 disables the layer entirely. */
  intensity: number;
  /**
   * Cells across one full azimuth turn at the horizon (roughly "stars across
   * the horizon" at full coverage). 64–256 is a reasonable range.
   */
  density: number;
  /** Fraction of cells that actually contain a star, in [0, 1]. */
  threshold: number;
  /** Star point size in local cell units. 0.02–0.1 works well. */
  size: number;
  /**
   * Fast twinkle rate in rad/s. Drives the high-frequency brightness
   * modulation. The low-frequency flicker-magnitude gate runs at a fixed
   * fraction of this rate internally. 0 disables twinkle.
   */
  twinkleSpeed: number;
  /**
   * Max brightness dip from twinkling, in [0, 1]. 0 = no twinkle, 1 = star
   * can drop to zero at peak flicker. Default 0.25 — subtle scintillation.
   */
  twinkleDepth?: number;
  /**
   * Elevation (after shared horizonOffset is applied) at which stars are fully
   * on. Default 0.04.
   */
  minElev?: number;
  /**
   * Half-width of the fade ramp around `minElev`. Default 0.03.
   */
  fadeRange?: number;
}
