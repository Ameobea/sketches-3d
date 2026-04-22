import type * as THREE from 'three';

export interface BuildingsLayerConfig {
  /** Primary warm tone for lit windows. */
  color?: THREE.ColorRepresentation;
  /** Secondary tone; each window hash-mixes between `color` and `colorAlt`. */
  colorAlt?: THREE.ColorRepresentation;
  /** Overall window-brightness multiplier. */
  intensity: number;
  /**
   * Number of azimuth slots around the full horizon. Each slot either contains
   * a building or is skipped based on `buildingPresence`. Higher = more,
   * narrower buildings. 200–800 is reasonable.
   */
  buildingCount: number;
  /** Fraction of slots that contain a building, in [0, 1]. Default 0.85. */
  buildingPresence?: number;
  /**
   * Fraction of each slot reserved as gap on either side of the body, in
   * [0, 1]. Default 0.15 — creates visible separation between towers.
   */
  buildingGap?: number;
  /**
   * Min / max vertical extent of a building in elevation units (asin-uniform).
   * 0.1 is a noticeable building at the horizon.
   */
  buildingMinHeight: number;
  buildingMaxHeight: number;
  /** Min / max number of floors per building. Default 4 / 16. */
  floorsMin?: number;
  floorsMax?: number;
  /** Min / max number of window columns per building. Default 2 / 6. */
  windowsMin?: number;
  windowsMax?: number;
  /** Max stride between lit floors / columns. Defaults 2 / 1. */
  maxFloorStride?: number;
  maxWindowStride?: number;
  /** Min / max fraction of eligible windows that are lit. Default 0.2 / 0.8. */
  litFractionMin?: number;
  litFractionMax?: number;
  /** Lit-window rectangle size as a fraction of its grid cell. Defaults 0.4 / 0.5. */
  windowWidth?: number;
  windowHeight?: number;
  /** Fast twinkle rate in rad/s. */
  twinkleSpeed: number;
  /** Max brightness dip from twinkling. Default 0.15. */
  twinkleDepth?: number;
  /**
   * Elevation (after the shared horizonOffset is applied) of the ground line
   * that all buildings rise from. Default 0.
   */
  groundElev?: number;
  /**
   * Multiplier on the sky gradient color to produce the silhouette body color.
   * 0 = pitch black, 1 = no darkening. Default 0.15.
   */
  silhouetteDarkness?: number;
}
