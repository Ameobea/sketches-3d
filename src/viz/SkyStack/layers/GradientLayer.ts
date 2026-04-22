import type * as THREE from 'three';

import type { CloudBand, GradientStop, HorizonMode } from '../uniforms';

export interface GradientLayerConfig {
  stops: GradientStop[];
  horizonMode?: HorizonMode;
  belowColor?: THREE.ColorRepresentation;
  horizonBlend?: number;
  bands?: CloudBand[];
}
