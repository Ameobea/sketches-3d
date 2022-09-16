import * as THREE from 'three';

import type { VizState } from '.';

export const initBaseScene = (viz: VizState) => {
  // Add lights
  const light = new THREE.DirectionalLight(0xcfcfcf, 1.5);
  light.position.set(80, 60, 80);
  viz.scene.add(light);

  // Add a cube at the position of the light
  // const lightCube = new THREE.Mesh(
  //   new THREE.BoxGeometry(10.1, 10.1, 10.1),
  //   new THREE.MeshBasicMaterial({ color: 0xffffff })
  // );
  // lightCube.position.copy(light.position);
  // viz.scene.add(lightCube);

  const ambientlight = new THREE.AmbientLight(0xe3d2d2, 0.05);
  viz.scene.add(ambientlight);
  return { ambientlight, light };
};

// Corresponds to GLSL function in `noise.frag`
const hash = (num: number) => {
  let p = num * 0.011;
  p = p - Math.floor(p);
  p *= p + 7.5;
  p *= p + p;
  return p - Math.floor(p);
};

// Corresponds to GLSL function in `noise.frag`
export const noise = (x: number) => {
  const i = Math.floor(x);
  const f = x - i;
  const u = f * f * (3 - 2 * f);
  return hash(i) * (1 - u) + hash(i + 1) * u;
};

export const smoothstep = (start: number, stop: number, x: number) => {
  const t = Math.max(0, Math.min(1, (x - start) / (stop - start)));
  return t * t * (3 - 2 * t);
};

// float flickerVal = noise(curTimeSeconds * 1.5);
// float flickerActivation = smoothstep(0.4, 1.0, flickerVal * 2. + 0.2);
// return flickerActivation;

export const getFlickerActivation = (curTimeSeconds: number) => {
  const flickerVal = noise(curTimeSeconds * 1.5);
  const flickerActivation = smoothstep(0.4, 1.0, flickerVal * 2 + 0.2);
  return flickerActivation;
};

export const delay = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

export const clamp = (val: number, min: number, max: number) => Math.min(Math.max(val, min), max);

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

export const DEVICE_PIXEL_RATIO = Math.min(window.devicePixelRatio || 1, 2);
