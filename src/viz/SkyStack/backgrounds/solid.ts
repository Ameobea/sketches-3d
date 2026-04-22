import * as THREE from 'three';

import type { BackgroundLayer } from '../types';

export interface SolidBackgroundConfig {
  /** Defaults to 'background_solid'. Override to avoid uniform-name collision. */
  id?: string;
  color: THREE.ColorRepresentation;
}

/** Flat solid-color background. Emits the color at alpha=1. */
export const solidBackground = (c: SolidBackgroundConfig): BackgroundLayer => {
  const id = c.id ?? 'background_solid';
  const uniforms: Record<string, THREE.IUniform> = {
    [`uSolidColor_${id}`]: { value: new THREE.Color(c.color) },
  };
  return {
    id,
    uniforms,
    instanceGlsl: `uniform vec3 uSolidColor_${id};`,
    body: `accumulate(uSolidColor_${id}, vec3(0.0), 1.0, 0.0);`,
  };
};
