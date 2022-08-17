import type { VizState } from '..';
import { initBaseScene } from '../util';

export const processLoadedScene = (viz: VizState, loadedWorld: THREE.Group) => {
  initBaseScene(viz);
};
