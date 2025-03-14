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

export const initEulerDebugger = (viz: VizState, container: HTMLElement, topPx: number) => {
  const eulerDisplayElem = createStatsContainer(topPx);
  container.appendChild(eulerDisplayElem);

  viz.registerBeforeRenderCb(() => {
    const euler = viz.camera.rotation;
    const x = euler.x.toFixed(3);
    const y = euler.y.toFixed(3);
    const z = euler.z.toFixed(3);
    eulerDisplayElem.innerText = `${x}, ${y}, ${z}`;
  });
};
