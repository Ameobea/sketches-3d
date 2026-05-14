// Run with: yarn tsx --test src/viz/gizmos/dragMath.test.ts
import assert from 'node:assert/strict';
import { test } from 'node:test';
import * as THREE from 'three';

import {
  closestPointOnLineParam,
  intersectRayPlane,
  ndcToRay,
  projectAxisDrag,
  projectAxisScaleFactor,
  projectPlaneDrag,
  projectRotateDrag,
  projectUniformScale,
} from './dragMath';

const makeCamera = (pos: [number, number, number], lookAt: [number, number, number]) => {
  const cam = new THREE.PerspectiveCamera(60, 1, 0.1, 1000);
  cam.position.set(...pos);
  cam.lookAt(new THREE.Vector3(...lookAt));
  cam.updateMatrixWorld();
  return cam;
};

const close = (a: number, b: number, eps = 1e-4) => Math.abs(a - b) < eps;

test('ndcToRay: centre of view points at the look target', () => {
  const cam = makeCamera([0, 0, 10], [0, 0, 0]);
  const ray = ndcToRay({ x: 0, y: 0 }, cam);
  assert.ok(ray.origin.distanceTo(new THREE.Vector3(0, 0, 10)) < 1e-5);
  assert.ok(ray.direction.distanceTo(new THREE.Vector3(0, 0, -1)) < 1e-5);
});

test('closestPointOnLineParam: returns null for line parallel to ray', () => {
  const linePt = new THREE.Vector3(1, 0, 0);
  const lineDir = new THREE.Vector3(0, 0, 1);
  const rayOrigin = new THREE.Vector3(0, 0, 0);
  const rayDir = new THREE.Vector3(0, 0, 1);
  assert.equal(closestPointOnLineParam(linePt, lineDir, rayOrigin, rayDir), null);
});

test('intersectRayPlane: simple hit and a parallel miss', () => {
  const ray = new THREE.Ray(new THREE.Vector3(0, 0, 5), new THREE.Vector3(0, 0, -1));
  const hit = intersectRayPlane(ray, new THREE.Vector3(0, 0, 0), new THREE.Vector3(0, 0, 1));
  assert.ok(hit && hit.distanceTo(new THREE.Vector3(0, 0, 0)) < 1e-6);

  const parallel = new THREE.Ray(new THREE.Vector3(0, 0, 1), new THREE.Vector3(1, 0, 0));
  assert.equal(intersectRayPlane(parallel, new THREE.Vector3(0, 0, 0), new THREE.Vector3(0, 0, 1)), null);
});

test('projectAxisDrag: dragging right along screen +X moves origin in +X (camera at +Z)', () => {
  const cam = makeCamera([0, 0, 10], [0, 0, 0]);
  const origin = new THREE.Vector3(0, 0, 0);
  const axis = new THREE.Vector3(1, 0, 0);
  const d = projectAxisDrag(cam, origin, axis, { x: 0, y: 0 }, { x: 0.2, y: 0 });
  assert.ok(d !== null && d > 0, `expected positive translation, got ${d}`);
  // Symmetric drag the other way is negated.
  const d2 = projectAxisDrag(cam, origin, axis, { x: 0, y: 0 }, { x: -0.2, y: 0 });
  assert.ok(d2 !== null && close(d2, -d!));
});

test('projectAxisDrag: ignores cursor motion perpendicular to the axis', () => {
  const cam = makeCamera([0, 0, 10], [0, 0, 0]);
  const origin = new THREE.Vector3(0, 0, 0);
  const axis = new THREE.Vector3(1, 0, 0);
  const d = projectAxisDrag(cam, origin, axis, { x: 0, y: 0 }, { x: 0, y: 0.4 });
  assert.ok(d !== null && Math.abs(d) < 1e-3, `expected ~0, got ${d}`);
});

test('projectAxisDrag: returns null when the camera looks straight down the axis', () => {
  const cam = makeCamera([10, 0, 0], [0, 0, 0]);
  const origin = new THREE.Vector3(0, 0, 0);
  const axis = new THREE.Vector3(1, 0, 0);
  const d = projectAxisDrag(cam, origin, axis, { x: 0, y: 0 }, { x: 0.2, y: 0 });
  assert.equal(d, null);
});

test('projectPlaneDrag: XZ plane handle with camera overhead → cursor maps to plane offset', () => {
  const cam = makeCamera([0, 10, 0], [0, 0, 0]);
  const origin = new THREE.Vector3(0, 0, 0);
  const normal = new THREE.Vector3(0, 1, 0);
  const delta = projectPlaneDrag(cam, origin, normal, { x: 0, y: 0 }, { x: 0.3, y: 0 });
  assert.ok(delta !== null);
  assert.ok(delta!.x > 0, `expected +X movement, got ${delta!.toArray()}`);
  assert.ok(Math.abs(delta!.y) < 1e-3);
});

test('projectRotateDrag: 90deg cursor sweep around Z axis = pi/2 rotation', () => {
  const cam = makeCamera([0, 0, 10], [0, 0, 0]);
  const origin = new THREE.Vector3(0, 0, 0);
  const axis = new THREE.Vector3(0, 0, 1);
  const angle = projectRotateDrag(cam, origin, axis, { x: 0.4, y: 0 }, { x: 0, y: 0.4 });
  assert.ok(angle !== null && close(angle!, Math.PI / 2, 1e-3), `got ${angle}`);
  // Reverse direction = negated angle.
  const angle2 = projectRotateDrag(cam, origin, axis, { x: 0.4, y: 0 }, { x: 0, y: -0.4 });
  assert.ok(angle2 !== null && close(angle2!, -Math.PI / 2, 1e-3));
});

test('projectAxisScaleFactor: doubling cursor distance from origin doubles scale', () => {
  const cam = makeCamera([0, 0, 10], [0, 0, 0]);
  const origin = new THREE.Vector3(0, 0, 0);
  const axis = new THREE.Vector3(1, 0, 0);
  const f = projectAxisScaleFactor(cam, origin, axis, { x: 0.2, y: 0 }, { x: 0.4, y: 0 });
  assert.ok(f !== null && close(f!, 2, 1e-3), `expected 2, got ${f}`);
});

test('projectUniformScale: cursor-from-centre distance ratio drives factor', () => {
  const cam = makeCamera([0, 0, 10], [0, 0, 0]);
  const origin = new THREE.Vector3(0, 0, 0);
  const f = projectUniformScale(cam, origin, { x: 0.1, y: 0 }, { x: 0.2, y: 0 });
  assert.ok(close(f, 2, 1e-3));
  const f2 = projectUniformScale(
    cam,
    origin,
    { x: 0.1, y: 0 },
    { x: Math.SQRT1_2 * 0.1, y: Math.SQRT1_2 * 0.1 }
  );
  assert.ok(close(f2, 1, 1e-3));
});
