import type { Viz } from '..';
import { initBaseScene } from '../util/three';

export const processLoadedScene = (viz: Viz, _loadedWorld: THREE.Group) => {
  initBaseScene(viz);
};
