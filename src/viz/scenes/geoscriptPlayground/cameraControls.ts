import type { RenderedObject } from 'src/geoscript/runner/types';
import type { Viz } from 'src/viz';
import * as THREE from 'three';
import { DefaultCameraPos, DefaultCameraTarget } from './types';

const computeCompositeBoundingBox = (objects: RenderedObject[]): THREE.Box3 => {
  const box = new THREE.Box3();
  for (const obj of objects) {
    if (!(obj instanceof THREE.Mesh || obj instanceof THREE.Line)) {
      continue;
    }

    obj.geometry.computeBoundingBox();
    const meshBox = obj.geometry.boundingBox;
    if (meshBox) box.union(meshBox.applyMatrix4(obj.matrixWorld));
  }
  return box;
};

export const centerView = async (viz: Viz, renderedObjects: RenderedObject[]) => {
  while (!viz.orbitControls) {
    await new Promise(resolve => setTimeout(resolve, 10));
  }

  if (!renderedObjects.length) {
    viz.camera.position.copy(DefaultCameraPos);
    viz.orbitControls!.target.copy(DefaultCameraTarget);
    viz.camera.lookAt(DefaultCameraTarget);
    viz.orbitControls!.update();
    return;
  }

  const compositeBbox = computeCompositeBoundingBox(renderedObjects);
  const boundingSphere = new THREE.Sphere();
  compositeBbox.getBoundingSphere(boundingSphere);
  let center = boundingSphere.center;
  let radius = boundingSphere.radius;

  if (Number.isNaN(center.x) || Number.isNaN(center.y) || Number.isNaN(center.z)) {
    center = new THREE.Vector3(0, 0, 0);
  }
  if (radius <= 0 || Number.isNaN(radius)) {
    radius = 1;
  }

  // try to keep the same look direction
  const lookDir = new THREE.Vector3();
  lookDir.copy(viz.camera.position).sub(viz.orbitControls!.target);

  if (lookDir.lengthSq() === 0) {
    lookDir.set(1, 1, 1);
  }
  lookDir.normalize();

  const camera = viz.camera as THREE.PerspectiveCamera;
  let distance;

  if (!camera.isPerspectiveCamera) {
    console.warn('centerView only works with PerspectiveCamera, falling back to old method');
    const size = new THREE.Vector3();
    compositeBbox.getSize(size);
    const maxDim = Math.max(size.x, size.y, size.z);
    distance = maxDim * 1.2 + 1;
  } else {
    const vfov = THREE.MathUtils.degToRad(camera.fov);
    const hfov = 2 * Math.atan(Math.tan(vfov / 2) * camera.aspect);
    const fov = Math.min(vfov, hfov);

    // Compute distance to fit bounding sphere in view
    distance = radius / Math.sin(fov / 2);

    // Add a little padding so the object is not touching the screen edge
    distance *= 1.1;
  }

  viz.camera.position.copy(center).add(lookDir.multiplyScalar(distance));
  viz.orbitControls!.target.copy(center);
  viz.camera.lookAt(center);
  viz.orbitControls!.update();
};

const AXES = {
  x: new THREE.Vector3(1, 0, 0),
  y: new THREE.Vector3(0, 1, 0),
  z: new THREE.Vector3(0, 0, 1),
} as const;

export const snapView = (viz: Viz, axis: 'x' | 'y' | 'z') => {
  if (!viz.orbitControls) {
    return;
  }

  const axisVec = AXES[axis];
  const viewDir = new THREE.Vector3().subVectors(viz.orbitControls.target, viz.camera.position).normalize();
  const dot = viewDir.dot(axisVec);

  let sideSign: 1 | -1 = 1;
  if (Math.abs(Math.abs(dot) - 1) < 1e-3) {
    sideSign = dot < 0 ? -1 : 1;
  }

  const distance = viz.camera.position.distanceTo(viz.orbitControls.target);

  viz.camera.position.copy(viz.orbitControls.target).addScaledVector(axisVec, distance * sideSign);
  viz.camera.lookAt(viz.orbitControls.target);
};

export const orbit = (viz: Viz, axis: 'vertical' | 'horizontal', angle: number) => {
  if (!viz.orbitControls) {
    return;
  }

  const camera = viz.camera;
  const target = viz.orbitControls.target;

  const offset = new THREE.Vector3().subVectors(camera.position, target);
  const s = new THREE.Spherical().setFromVector3(offset);

  if (axis === 'horizontal') {
    s.theta += angle;

    const minAz = viz.orbitControls.minAzimuthAngle ?? -Infinity;
    const maxAz = viz.orbitControls.maxAzimuthAngle ?? Infinity;
    s.theta = Math.max(minAz, Math.min(maxAz, s.theta));
  } else {
    s.phi += angle;

    const minPol = viz.orbitControls.minPolarAngle ?? 0;
    const maxPol = viz.orbitControls.maxPolarAngle ?? Math.PI;
    s.phi = Math.max(minPol, Math.min(maxPol, s.phi));
  }

  offset.setFromSpherical(s);
  camera.position.copy(target).add(offset);
  camera.lookAt(target);

  viz.orbitControls.update();
};
