import * as THREE from 'three';

/** Debug material override for all rendered meshes (matches the `n` / `w` / `shift+w` keybinds). */
export type MaterialOverrideMode = 'wireframe' | 'wireframe-xray' | 'normal';

export const DefaultCameraPos = new THREE.Vector3(10, 10, 10);
export const DefaultCameraTarget = new THREE.Vector3(0, 0, 0);
export const DefaultCameraFOV = 60;
export const DefaultCameraZoom = 1;

export const IntFormatter = new Intl.NumberFormat(undefined, {
  style: 'decimal',
  maximumFractionDigits: 0,
});
