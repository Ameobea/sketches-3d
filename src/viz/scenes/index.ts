import type { VizState } from '..';

export const SceneLoadersBySceneName: {
  [key: string]: () => Promise<(viz: VizState, loadedWorld: THREE.Group) => void | Promise<void>>;
} = {
  bridge: () => import('./bridge').then(mod => mod.processLoadedScene),
  blink: () => import('./blink').then(mod => mod.processLoadedScene),
};
