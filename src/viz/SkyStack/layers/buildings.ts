import * as THREE from 'three';

import type { Layer } from '../types';
import { resolveId } from './_util';
import buildingsGlsl from './buildings.glsl?raw';

export interface BuildingsLayerConfig {
  id: string;
  zIndex: number;
  /** Primary warm tone for lit windows. */
  color?: THREE.ColorRepresentation;
  /** Secondary tone; each window hash-mixes between `color` and `colorAlt`. */
  colorAlt?: THREE.ColorRepresentation;
  /** Overall window-brightness multiplier. */
  intensity: number;
  /** Azimuth slots around the full horizon. Higher = more, narrower buildings. */
  buildingCount: number;
  /** Fraction of slots that contain a building, in [0, 1]. Default 0.85. */
  buildingPresence?: number;
  /** Fraction of each slot reserved as gap, in [0, 1]. Default 0.15. */
  buildingGap?: number;
  /** Min / max vertical extent of a building in elevation units. */
  buildingMinHeight: number;
  buildingMaxHeight: number;
  floorsMin?: number;
  floorsMax?: number;
  windowsMin?: number;
  windowsMax?: number;
  maxFloorStride?: number;
  maxWindowStride?: number;
  litFractionMin?: number;
  litFractionMax?: number;
  windowWidth?: number;
  windowHeight?: number;
  /** Fast twinkle rate in rad/s. */
  twinkleSpeed: number;
  /** Max brightness dip from twinkling. Default 0.15. */
  twinkleDepth?: number;
  /** Elevation of the ground line buildings rise from. Default 0. */
  groundElev?: number;
  /**
   * Flat silhouette color for occluded-sky pixels inside a building body.
   * Pick a darkened value roughly matching your backgrounds horizon color
   * to blend buildings into the sky.
   */
  silhouetteColor: THREE.ColorRepresentation;
}

export const buildingsLayer = (c: BuildingsLayerConfig): Layer => {
  const id = c.id;
  const uniforms: Record<string, THREE.IUniform> = {
    [`uBuildingCount_${id}`]: { value: c.buildingCount },
    [`uBuildingPresence_${id}`]: { value: c.buildingPresence ?? 0.85 },
    [`uBuildingGap_${id}`]: { value: c.buildingGap ?? 0.15 },
    [`uBuildingMinHeight_${id}`]: { value: c.buildingMinHeight },
    [`uBuildingMaxHeight_${id}`]: { value: c.buildingMaxHeight },
    [`uFloorsMin_${id}`]: { value: c.floorsMin ?? 4 },
    [`uFloorsMax_${id}`]: { value: c.floorsMax ?? 16 },
    [`uWindowsMin_${id}`]: { value: c.windowsMin ?? 2 },
    [`uWindowsMax_${id}`]: { value: c.windowsMax ?? 6 },
    [`uMaxFloorStride_${id}`]: { value: c.maxFloorStride ?? 2 },
    [`uMaxWindowStride_${id}`]: { value: c.maxWindowStride ?? 1 },
    [`uLitFractionMin_${id}`]: { value: c.litFractionMin ?? 0.2 },
    [`uLitFractionMax_${id}`]: { value: c.litFractionMax ?? 0.8 },
    [`uGroundElev_${id}`]: { value: c.groundElev ?? 0.0 },
    [`uWindowWidth_${id}`]: { value: c.windowWidth ?? 0.4 },
    [`uWindowHeight_${id}`]: { value: c.windowHeight ?? 0.5 },
    [`uCityColor_${id}`]: { value: new THREE.Color(c.color ?? 0xffb070) },
    [`uCityColorAlt_${id}`]: { value: new THREE.Color(c.colorAlt ?? 0xffd89a) },
    [`uCityIntensity_${id}`]: { value: c.intensity },
    [`uBuildingTwinkleSpeed_${id}`]: { value: c.twinkleSpeed },
    [`uBuildingTwinkleDepth_${id}`]: { value: c.twinkleDepth ?? 0.15 },
    [`uSilhouetteColor_${id}`]: { value: new THREE.Color(c.silhouetteColor) },
  };

  return {
    id,
    zIndex: c.zIndex,
    uniforms,
    instanceGlsl: resolveId(buildingsGlsl, id),
    body: resolveId(
      `BuildingHit_$ID hit = probeBuilding_$ID(elev, azimuth);
      if (hit.hasBody) {
        // Opaque silhouette (alpha=1) — saturates the stack and short-circuits
        // any layer behind. Windows ride the emissive channel.
        vec4 win = sampleWindows_$ID(hit);
        accumulate(uSilhouetteColor_$ID, win.rgb, 1.0, win.a);
      }`,
      id
    ),
    gate: 'aboveHorizon',
  };
};
