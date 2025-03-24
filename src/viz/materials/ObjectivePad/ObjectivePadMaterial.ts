import * as THREE from 'three';

import { buildCustomShader } from 'src/viz/shaders/customShader';
import colorShader from './shaders/color.frag?raw';

export const ObjectivePadMaterial = buildCustomShader(
  { metalness: 0, alphaTest: 0.05, transparent: true, side: THREE.DoubleSide },
  { colorShader },
  { disableToneMapping: true }
);
