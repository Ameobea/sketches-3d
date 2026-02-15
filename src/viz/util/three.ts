import * as THREE from 'three';

import type { Viz } from '..';

export const initBaseScene = (viz: Viz) => {
  const light = new THREE.DirectionalLight(0xcfcfcf, 1.5);
  light.position.set(80, 60, 80);
  viz.scene.add(light);

  const ambientlight = new THREE.AmbientLight(0xe3d2d2, 0.05);
  viz.scene.add(ambientlight);
  return { ambientlight, light };
};

export const getMesh = (group: THREE.Group, name: string): THREE.Mesh => {
  const maybeMesh = group.getObjectByName(name);
  if (!maybeMesh) {
    throw new Error(`Could not find mesh with name ${name}`);
  }

  if (maybeMesh instanceof THREE.Mesh) {
    return maybeMesh;
  } else if (maybeMesh.children.length > 0) {
    if (maybeMesh.children.length !== 1) {
      throw new Error(`Expected group ${name} to have 1 child`);
    }

    const child = maybeMesh.children[0];
    if (!(child instanceof THREE.Mesh)) {
      throw new Error(`Expected group ${name} to have a mesh child`);
    }

    return child;
  } else {
    console.error(maybeMesh);
    throw new Error(`Expected mesh or group with name ${name}`);
  }
};
