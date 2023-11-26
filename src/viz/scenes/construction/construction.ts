import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import type { SceneConfig } from '..';

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConfig: VizConfig
): Promise<SceneConfig> => {
  viz.scene.background = new THREE.Color(0x030303);
  viz.scene.add(new THREE.AmbientLight(0xffffff, 1.5));
  viz.camera.near = 0.1;
  viz.camera.far = 500;
  viz.camera.updateProjectionMatrix();

  configureDefaultPostprocessingPipeline(viz, vizConfig.graphics.quality);

  return {
    viewMode: { type: 'firstPerson' },
    locations: {
      spawn: {
        pos: [9.41042709350586, 0.0, -0.8715839385986328],
        rot: [0.064, 1.596, 0],
      },
    },
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 19, inAir: 19 },
      colliderCapsuleSize: { height: 6.2, radius: 0.8 },
      jumpVelocity: 16,
      oobYThreshold: -50,
    },
    debugPos: true,
  };
};
