import * as THREE from 'three';

/**
 * Drag projection math for the custom gizmo.
 *
 * Conventions: vectors are world-space; mouse positions are NDC (x, y in
 * [-1, 1], +y up); axis / normal arguments are unit length.
 */

const EPS = 1e-8;

/** Build a world-space ray from the given camera and NDC mouse position. */
export const ndcToRay = (
  ndc: { x: number; y: number },
  camera: THREE.Camera,
  out: THREE.Ray = new THREE.Ray()
): THREE.Ray => {
  const o = out.origin;
  const d = out.direction;
  o.set(ndc.x, ndc.y, -1).unproject(camera);
  d.set(ndc.x, ndc.y, 1).unproject(camera).sub(o).normalize();
  // Perspective rays start at the camera; for ortho the near-plane point from
  // unproject(-1) is already correct.
  if ((camera as THREE.PerspectiveCamera).isPerspectiveCamera) {
    o.setFromMatrixPosition(camera.matrixWorld);
  }
  return out;
};

/** Parameter `s` for the closest point `linePoint + s * lineDir` to the ray. */
export const closestPointOnLineParam = (
  linePoint: THREE.Vector3,
  lineDir: THREE.Vector3,
  rayOrigin: THREE.Vector3,
  rayDir: THREE.Vector3
): number | null => {
  const b = lineDir.dot(rayDir);
  const det = 1 - b * b;
  if (Math.abs(det) < EPS) {
    return null;
  }
  const dx = rayOrigin.x - linePoint.x;
  const dy = rayOrigin.y - linePoint.y;
  const dz = rayOrigin.z - linePoint.z;
  const dDotU = dx * lineDir.x + dy * lineDir.y + dz * lineDir.z;
  const dDotV = dx * rayDir.x + dy * rayDir.y + dz * rayDir.z;
  return (dDotU - b * dDotV) / det;
};

/** Intersect a ray with a plane.  Returns `null` when parallel. */
export const intersectRayPlane = (
  ray: THREE.Ray,
  planePoint: THREE.Vector3,
  planeNormal: THREE.Vector3,
  out: THREE.Vector3 = new THREE.Vector3()
): THREE.Vector3 | null => {
  const denom = ray.direction.dot(planeNormal);
  if (Math.abs(denom) < EPS) return null;
  const dx = planePoint.x - ray.origin.x;
  const dy = planePoint.y - ray.origin.y;
  const dz = planePoint.z - ray.origin.z;
  const t = (dx * planeNormal.x + dy * planeNormal.y + dz * planeNormal.z) / denom;
  if (!Number.isFinite(t)) return null;
  return out.copy(ray.direction).multiplyScalar(t).add(ray.origin);
};

/**
 * Signed world-space translation along `axisDir` for a cursor drag from
 * `ndcStart` to `ndcNow`.  `null` when the camera looks too close to straight
 * down the axis (caller should skip the frame).
 */
export const projectAxisDrag = (
  camera: THREE.Camera,
  axisOrigin: THREE.Vector3,
  axisDir: THREE.Vector3,
  ndcStart: { x: number; y: number },
  ndcNow: { x: number; y: number }
): number | null => {
  const rayStart = ndcToRay(ndcStart, camera, _scratchRayA);
  const sStart = closestPointOnLineParam(axisOrigin, axisDir, rayStart.origin, rayStart.direction);
  if (sStart === null) return null;
  const rayNow = ndcToRay(ndcNow, camera, _scratchRayB);
  const sNow = closestPointOnLineParam(axisOrigin, axisDir, rayNow.origin, rayNow.direction);
  if (sNow === null) return null;
  return sNow - sStart;
};

/** In-plane translation vector `(P_now - P_start)`.  `null` if either ray ∥ plane. */
export const projectPlaneDrag = (
  camera: THREE.Camera,
  planePoint: THREE.Vector3,
  planeNormal: THREE.Vector3,
  ndcStart: { x: number; y: number },
  ndcNow: { x: number; y: number },
  out: THREE.Vector3 = new THREE.Vector3()
): THREE.Vector3 | null => {
  const rayStart = ndcToRay(ndcStart, camera, _scratchRayA);
  const hitStart = intersectRayPlane(rayStart, planePoint, planeNormal, _scratchVecA);
  if (!hitStart) return null;
  const rayNow = ndcToRay(ndcNow, camera, _scratchRayB);
  const hitNow = intersectRayPlane(rayNow, planePoint, planeNormal, _scratchVecB);
  if (!hitNow) return null;
  return out.copy(hitNow).sub(hitStart);
};

/**
 * Right-handed rotation angle (radians) around `axisDir` through `axisOrigin`,
 * measured by intersecting both rays with the perpendicular plane.  `null` if
 * the axis is nearly aligned with the view direction.
 */
export const projectRotateDrag = (
  camera: THREE.Camera,
  axisOrigin: THREE.Vector3,
  axisDir: THREE.Vector3,
  ndcStart: { x: number; y: number },
  ndcNow: { x: number; y: number }
): number | null => {
  const rayStart = ndcToRay(ndcStart, camera, _scratchRayA);
  const hitStart = intersectRayPlane(rayStart, axisOrigin, axisDir, _scratchVecA);
  if (!hitStart) return null;
  const rayNow = ndcToRay(ndcNow, camera, _scratchRayB);
  const hitNow = intersectRayPlane(rayNow, axisOrigin, axisDir, _scratchVecB);
  if (!hitNow) return null;

  // Orthonormal basis in the rotation plane.  Hint with world-up; swap to +X
  // when axis ∥ up.
  const u = _scratchVecC.set(0, 1, 0);
  if (Math.abs(u.dot(axisDir)) > 0.99) u.set(1, 0, 0);
  const e1 = _scratchVecD.copy(u).cross(axisDir).normalize();
  const e2 = _scratchVecE.copy(axisDir).cross(e1).normalize();

  const sx = hitStart.x - axisOrigin.x;
  const sy = hitStart.y - axisOrigin.y;
  const sz = hitStart.z - axisOrigin.z;
  const nx = hitNow.x - axisOrigin.x;
  const ny = hitNow.y - axisOrigin.y;
  const nz = hitNow.z - axisOrigin.z;

  const aStart = Math.atan2(sx * e2.x + sy * e2.y + sz * e2.z, sx * e1.x + sy * e1.y + sz * e1.z);
  const aNow = Math.atan2(nx * e2.x + ny * e2.y + nz * e2.z, nx * e1.x + ny * e1.y + nz * e1.z);

  let d = aNow - aStart;
  if (d > Math.PI) d -= 2 * Math.PI;
  else if (d < -Math.PI) d += 2 * Math.PI;
  return d;
};

/**
 * Like `projectAxisDrag` but returns the ratio `sNow / sStart` so half-distance
 * = 0.5× scale.  `null` on degenerate projections or zero `sStart`.
 */
export const projectAxisScaleFactor = (
  camera: THREE.Camera,
  axisOrigin: THREE.Vector3,
  axisDir: THREE.Vector3,
  ndcStart: { x: number; y: number },
  ndcNow: { x: number; y: number }
): number | null => {
  const rayStart = ndcToRay(ndcStart, camera, _scratchRayA);
  const sStart = closestPointOnLineParam(axisOrigin, axisDir, rayStart.origin, rayStart.direction);
  if (sStart === null) return null;
  if (Math.abs(sStart) < EPS) return null;
  const rayNow = ndcToRay(ndcNow, camera, _scratchRayB);
  const sNow = closestPointOnLineParam(axisOrigin, axisDir, rayNow.origin, rayNow.direction);
  if (sNow === null) return null;
  return sNow / sStart;
};

/** Uniform-scale factor from screen-space distance ratio around the origin. */
export const projectUniformScale = (
  camera: THREE.Camera,
  origin: THREE.Vector3,
  ndcStart: { x: number; y: number },
  ndcNow: { x: number; y: number }
): number => {
  const center = _scratchVecA.copy(origin).project(camera);
  const dxStart = ndcStart.x - center.x;
  const dyStart = ndcStart.y - center.y;
  const dxNow = ndcNow.x - center.x;
  const dyNow = ndcNow.y - center.y;
  const rStart = Math.hypot(dxStart, dyStart);
  const rNow = Math.hypot(dxNow, dyNow);
  if (rStart < EPS) return 1;
  return rNow / rStart;
};

const _scratchRayA = new THREE.Ray();
const _scratchRayB = new THREE.Ray();
const _scratchVecA = new THREE.Vector3();
const _scratchVecB = new THREE.Vector3();
const _scratchVecC = new THREE.Vector3();
const _scratchVecD = new THREE.Vector3();
const _scratchVecE = new THREE.Vector3();
