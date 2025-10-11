import * as THREE from 'three';

import type { RenderedObject } from 'src/geoscript/runner/types';
import type { Viz } from 'src/viz';

export const toggleAxisHelpers = (viz: Viz) => {
  const helper = viz.scene.children.find(obj => obj instanceof THREE.AxesHelper);
  if (helper) {
    viz.scene.remove(helper);
    localStorage['geoscript-axis-helpers'] = 'false';
  } else {
    const axisHelper = new THREE.AxesHelper(100);
    axisHelper.position.set(0, 0, 0);
    viz.scene.add(axisHelper);
    localStorage['geoscript-axis-helpers'] = 'true';
  }
};

export const buildLightHelpers = (viz: Viz, renderedObjects: RenderedObject[]): THREE.Object3D[] => {
  const helpers: THREE.Object3D[] = [];
  for (const obj of renderedObjects) {
    if (obj instanceof THREE.DirectionalLight) {
      const helper = new THREE.DirectionalLightHelper(obj, 5);
      viz.scene.add(helper);
      helpers.push(helper);
      if (obj.castShadow) {
        const shadowHelper = new THREE.CameraHelper(obj.shadow.camera);
        viz.scene.add(shadowHelper);
        helpers.push(shadowHelper);
      }
    } else if (obj instanceof THREE.PointLight) {
      const helper = new THREE.PointLightHelper(obj, 1);
      viz.scene.add(helper);
      helpers.push(helper);
    } else if (obj instanceof THREE.SpotLight) {
      const helper = new THREE.SpotLightHelper(obj);
      viz.scene.add(helper);
      helpers.push(helper);
    }
  }
  return helpers;
};

export const toggleLightHelpers = (
  viz: Viz,
  renderedObjects: RenderedObject[],
  lightHelpers: THREE.Object3D[]
): THREE.Object3D[] => {
  const lightHelpersEnabled = localStorage['geoscript-light-helpers'] === 'true';
  if (lightHelpersEnabled) {
    for (const helper of lightHelpers) {
      viz.scene.remove(helper);
    }
    localStorage['geoscript-light-helpers'] = 'false';
    return [];
  } else {
    const newLightHelpers = buildLightHelpers(viz, renderedObjects);
    localStorage['geoscript-light-helpers'] = 'true';
    return newLightHelpers;
  }
};
