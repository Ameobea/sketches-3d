import * as THREE from 'three';

import type { VizState } from '.';

export const initBaseScene = (viz: VizState) => {
  // Add lights
  const light = new THREE.DirectionalLight(0xcfcfcf, 1.5);
  light.position.set(80, 60, 80);
  viz.scene.add(light);

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

export const smoothstepScale = (start: number, stop: number, x: number, min: number, max: number) =>
  min + (max - min) * smoothstep(start, stop, x);

export const mix = (x: number, y: number, a: number) => x * (1 - a) + y * a;

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

export const DEVICE_PIXEL_RATIO = (() => {
  if (typeof window === 'undefined') {
    return 1;
  }
  return Math.min(window.devicePixelRatio || 1, 2);
})();

export const hasWasmSIMDSupport = async () =>
  WebAssembly.validate(
    new Uint8Array([
      0, 97, 115, 109, 1, 0, 0, 0, 1, 5, 1, 96, 0, 1, 123, 3, 2, 1, 0, 10, 10, 1, 8, 0, 65, 0, 253, 15, 253,
      98, 11,
    ])
  );

export const mergeDeep = <T extends Record<string, any>>(base: T, rhs: any): T => {
  const isObject = (obj: any) => obj && typeof obj === 'object';

  if (!isObject(base) || !isObject(rhs)) {
    return rhs;
  }

  Object.keys(rhs).forEach(key => {
    const targetValue = base[key];
    const sourceValue = rhs[key];

    if (Array.isArray(targetValue) && Array.isArray(sourceValue)) {
      (base as any)[key] = targetValue.concat(sourceValue);
    } else if (isObject(targetValue) && isObject(sourceValue)) {
      (base as any)[key] = mergeDeep(Object.assign({}, targetValue), sourceValue);
    } else {
      (base as any)[key] = sourceValue;
    }
  });

  return base;
};

export const randomInRange = (min: number, max: number) => Math.random() * (max - min) + min;

export type DeepPartial<T> = T extends object
  ? {
      [P in keyof T]?: DeepPartial<T[P]>;
    }
  : T;
