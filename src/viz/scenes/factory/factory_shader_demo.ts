import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { GraphicsQuality } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import type { SceneConfig } from '..';
import { buildFactorySkyStack } from './skyStack';

export const processLoadedScene = (viz: Viz, _loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  const skyStack = buildFactorySkyStack(viz, vizConf);

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    toneMapping: { mode: 'agx', exposure: 1.2 },
    emissiveBypass: true,
    skyBypassTonemap: false,
    skyStack,
    emissiveBloom:
      vizConf.graphics.quality > GraphicsQuality.Low
        ? { intensity: 6.0, levels: 3, luminanceThreshold: 0.02, radius: 0.45, luminanceSoftKnee: 0.02 }
        : null,
  });

  return {
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 5, 0),
        rot: new THREE.Vector3(0, 0, 0),
      },
    },
    spawnLocation: 'spawn',
    viewMode: {
      type: 'orbit',
      pos: new THREE.Vector3(40, 15, 40),
      target: new THREE.Vector3(0, 0, 0),
    },
  };
};
