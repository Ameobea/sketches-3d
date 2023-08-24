import * as THREE from 'three';

import type { VizState } from '..';
import { createStatsContainer } from './statsContainer';

export const initTargetDebugger = (viz: VizState, container: HTMLElement, topPx: number) => {
  const targetDisplayElem = createStatsContainer(topPx);
  container.appendChild(targetDisplayElem);

  // Little "+" crosshair in the center of the screen
  const crosshairContainer = document.createElement('div');
  crosshairContainer.style.position = 'absolute';
  crosshairContainer.style.left = '50%';
  crosshairContainer.style.top = '50%';
  crosshairContainer.style.transform = 'translate(-50%, -50%)';
  crosshairContainer.style.color = 'white';
  crosshairContainer.style.fontSize = '24px';
  crosshairContainer.style.fontFamily = 'monospace';
  crosshairContainer.style.padding = '4px';
  crosshairContainer.style.zIndex = '1';
  crosshairContainer.innerText = '+';
  container.appendChild(crosshairContainer);

  const raycaster = new THREE.Raycaster();
  viz.registerBeforeRenderCb(() => {
    raycaster.setFromCamera(new THREE.Vector2(0, 0), viz.camera);
    const intersects = raycaster.intersectObjects(viz.scene.children);
    if (intersects.length > 0) {
      const target = intersects[0].object;
      targetDisplayElem.innerText = target.name;
    }
  });
};
