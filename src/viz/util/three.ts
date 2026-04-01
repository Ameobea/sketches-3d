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

/**
 * Temporarily exposes an object's transform as world-space position/quaternion/scale
 * for code that reads those properties directly instead of `matrixWorld`.
 */
export const withWorldSpaceTransform = <T extends THREE.Object3D, R>(object: T, cb: (object: T) => R): R => {
  object.updateWorldMatrix(true, false);

  const origPos = object.position.clone();
  const origQuat = object.quaternion.clone();
  const origScale = object.scale.clone();

  object.matrixWorld.decompose(object.position, object.quaternion, object.scale);

  try {
    return cb(object);
  } finally {
    object.position.copy(origPos);
    object.quaternion.copy(origQuat);
    object.scale.copy(origScale);
  }
};
