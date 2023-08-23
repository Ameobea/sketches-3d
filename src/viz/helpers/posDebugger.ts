import type { VizState } from '..';

export const initPosDebugger = (viz: VizState, container: HTMLElement) => {
  const posDisplayElem = document.createElement('div');
  posDisplayElem.style.position = 'absolute';
  posDisplayElem.style.top = '0px';
  posDisplayElem.style.right = '0px';
  posDisplayElem.style.color = 'white';
  posDisplayElem.style.fontSize = '12px';
  posDisplayElem.style.fontFamily = 'monospace';
  posDisplayElem.style.padding = '4px';
  posDisplayElem.style.backgroundColor = 'rgba(0, 0, 0, 0.5)';
  posDisplayElem.style.zIndex = '1';
  container.appendChild(posDisplayElem);

  viz.registerBeforeRenderCb(() => {
    const x = viz.camera.position.x.toFixed(2);
    const y = viz.camera.position.y.toFixed(2);
    const z = viz.camera.position.z.toFixed(2);
    posDisplayElem.innerText = `${x}, ${y}, ${z}`;
  });
};
