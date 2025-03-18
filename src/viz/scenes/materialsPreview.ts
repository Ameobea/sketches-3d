import type { Viz } from '..';
import { initBaseScene } from '../util/util';

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group) => {
  initBaseScene(viz);
};
