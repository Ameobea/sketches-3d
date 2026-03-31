import * as THREE from 'three';
import type { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';

export interface FocusCameraParams {
  camera: THREE.PerspectiveCamera;
  orbitControls: OrbitControls;
  center: THREE.Vector3;
  radius: number;
  /** Duration of the animation in milliseconds. Set to 0 to skip animation. Default: 150 */
  animationDurationMs?: number;
  /** Multiplier on the computed fit distance for padding. Default: 1.1 */
  paddingFactor?: number;
}

/**
 * Computes the distance needed to fit a bounding sphere of the given radius
 * inside a perspective camera's view frustum.
 */
export const computeFitDistance = (
  camera: THREE.PerspectiveCamera,
  radius: number,
  paddingFactor = 1.1
): number => {
  const vfov = THREE.MathUtils.degToRad(camera.fov);
  const hfov = 2 * Math.atan(Math.tan(vfov / 2) * camera.aspect);
  const fov = Math.min(vfov, hfov);
  return (radius / Math.sin(fov / 2)) * paddingFactor;
};

/**
 * Moves the orbit camera to focus on a point with a distance computed from a
 * bounding sphere radius.  Optionally animates the transition.
 */
export const focusCamera = (params: FocusCameraParams) => {
  const { camera, orbitControls, center, radius, animationDurationMs = 150, paddingFactor = 1.1 } = params;

  const distance = computeFitDistance(camera, radius, paddingFactor);

  // Preserve current look direction
  const lookDir = new THREE.Vector3().subVectors(camera.position, orbitControls.target);
  if (lookDir.lengthSq() === 0) {
    lookDir.set(1, 1, 1);
  }
  lookDir.normalize();

  const newTarget = center.clone();
  const newPosition = center.clone().add(lookDir.multiplyScalar(distance));

  if (animationDurationMs <= 0) {
    orbitControls.target.copy(newTarget);
    camera.position.copy(newPosition);
    orbitControls.update();
    return;
  }

  const startTarget = orbitControls.target.clone();
  const startPosition = camera.position.clone();
  const startTime = performance.now();

  const animate = () => {
    const elapsed = performance.now() - startTime;
    const t = Math.min(elapsed / animationDurationMs, 1);
    // Smooth-step easing (ease-in-out)
    const s = t * t * (3 - 2 * t);

    orbitControls.target.lerpVectors(startTarget, newTarget, s);
    camera.position.lerpVectors(startPosition, newPosition, s);
    orbitControls.update();

    if (t < 1) {
      requestAnimationFrame(animate);
    }
  };

  requestAnimationFrame(animate);
};
