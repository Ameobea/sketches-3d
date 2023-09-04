import * as THREE from 'three';

import type { VizState } from 'src/viz';
import type { SceneConfig } from '..';

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  viz.scene.add(new THREE.AmbientLight(0xffffff, 0.5));
  console.log(loadedWorld.children);

  return {
    viewMode: {
      type: 'firstPerson',
    },
    locations: {
      spawn: {
        pos: new THREE.Vector3(0, 2, 0),
        rot: new THREE.Vector3(),
      },
    },
    spawnLocation: 'spawn',
  };
};
