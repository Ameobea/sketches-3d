import type { RenderedObject } from 'src/geoscript/runner/types';
import type { Viz } from 'src/viz';
import * as THREE from 'three';
import { DefaultCameraPos, DefaultCameraTarget } from './types';
import { focusCamera } from 'src/viz/util/focusCamera';

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

  focusCamera({
    camera: viz.camera as THREE.PerspectiveCamera,
    orbitControls: viz.orbitControls!,
    center,
    radius,
    animationDurationMs: 0,
  });
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
