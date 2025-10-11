import type { Viz } from '..';
import { initBaseScene } from '../util/util';

export const processLoadedScene = (viz: Viz, _loadedWorld: THREE.Group) => {
  initBaseScene(viz);
};
