import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import { initBaseScene } from '../util';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 15, 0),
    rot: new THREE.Vector3(0, 0, 0),
  },
};

export const processLoadedScene = (viz: VizState, loadedWorld: THREE.Group): SceneConfig => {
  initBaseScene(viz);

  const cube = new THREE.Mesh(
    new THREE.BoxGeometry(100, 10, 100),
    new THREE.MeshBasicMaterial({ color: 0x00ff00 })
  );
  cube.position.set(0, 0, 0);
  loadedWorld.add(cube);

  return { locations, spawnLocation: 'spawn', debugPos: true };
};
