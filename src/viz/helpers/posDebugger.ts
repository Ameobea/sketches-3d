import type { VizState } from '..';
import { createStatsContainer } from './statsContainer';

export const initPosDebugger = (viz: VizState, container: HTMLElement, topPx: number) => {
  const posDisplayElem = createStatsContainer(topPx);
  container.appendChild(posDisplayElem);

  viz.registerBeforeRenderCb(() => {
    const x = viz.camera.position.x.toFixed(2);
    const y = viz.camera.position.y.toFixed(2);
    const z = viz.camera.position.z.toFixed(2);
    posDisplayElem.innerText = `${x}, ${y}, ${z}`;
  });
};
