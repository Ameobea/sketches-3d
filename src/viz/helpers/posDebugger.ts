import type { Viz } from '..';
import { createStatsContainer } from './statsContainer';

export const initPosDebugger = (viz: Viz, container: HTMLElement, topPx: number) => {
  const posDisplayElem = createStatsContainer(topPx);
  container.appendChild(posDisplayElem);

  let lastX = -Infinity;
  let lastY = -Infinity;
  let lastZ = -Infinity;
  viz.registerBeforeRenderCb(() => {
    const x = viz.camera.position.x;
    const y = viz.camera.position.y;
    const z = viz.camera.position.z;
    if (x === lastX && y === lastY && z === lastZ) {
      return;
    }
    lastX = x;
    lastY = y;
    lastZ = z;

    posDisplayElem.innerText = `${x.toFixed(2)}, ${y.toFixed(2)}, ${z.toFixed(2)}`;
  });

  return posDisplayElem;
};

export const initEulerDebugger = (viz: Viz, container: HTMLElement, topPx: number) => {
  const eulerDisplayElem = createStatsContainer(topPx);
  container.appendChild(eulerDisplayElem);

  let lastX = -Infinity;
  let lastY = -Infinity;
  let lastZ = -Infinity;
  viz.registerBeforeRenderCb(() => {
    const euler = viz.camera.rotation;
    const x = euler.x;
    const y = euler.y;
    const z = euler.z;
    if (x === lastX && y === lastY && z === lastZ) {
      return;
    }
    lastX = x;
    lastY = y;
    lastZ = z;

    eulerDisplayElem.innerText = `${x.toFixed(3)}, ${y.toFixed(3)}, ${z.toFixed(3)}`;
  });

  return eulerDisplayElem;
};
