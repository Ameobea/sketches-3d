import * as THREE from 'three';

/**
 * Two parallel geometry families per handle:
 *   - visual  — thin, high-segment for SDF materials in `gizmoMaterials.ts`.
 *   - picker  — fatter, lower-segment, invisible; forgiving raycast target.
 *
 * Sizes are tuned for unit gizmo length; the caller `setScalar`s the whole
 * gizmo Object3D for camera-relative sizing.
 */

export interface ShaftOpts {
  length?: number;
  radius?: number;
  segments?: number;
}

export const buildShaftGeometry = (opts: ShaftOpts = {}): THREE.BufferGeometry => {
  const length = opts.length ?? 0.85;
  const radius = opts.radius ?? 0.02;
  const segments = opts.segments ?? 12;
  // CylinderGeometry centres on the origin; translate so it runs y=0..length.
  const geo = new THREE.CylinderGeometry(radius, radius, length, segments, 1, true);
  geo.translate(0, length / 2, 0);
  return geo;
};

export const buildShaftPickerGeometry = (opts: ShaftOpts = {}): THREE.BufferGeometry =>
  buildShaftGeometry({
    length: opts.length ?? 1.0,
    // Tight enough that adjacent axes don't fight for the pick.
    radius: (opts.radius ?? 0.02) * 2.25,
    segments: opts.segments ?? 8,
  });

export interface ArrowheadOpts {
  length?: number;
  radius?: number;
  radialSegments?: number;
}

export const buildArrowheadGeometry = (opts: ArrowheadOpts = {}): THREE.BufferGeometry => {
  const length = opts.length ?? 0.18;
  const radius = opts.radius ?? 0.06;
  const radialSegments = opts.radialSegments ?? 12;
  const geo = new THREE.ConeGeometry(radius, length, radialSegments, 1, false);
  geo.translate(0, length / 2, 0);
  return geo;
};

export const buildArrowheadPickerGeometry = (opts: ArrowheadOpts = {}): THREE.BufferGeometry =>
  buildArrowheadGeometry({
    length: opts.length ?? 0.22,
    radius: (opts.radius ?? 0.06) * 1.35,
    radialSegments: opts.radialSegments ?? 8,
  });

export interface RingOpts {
  outerRadius?: number;
  segments?: number;
}

/** Flat XY disc; the band is drawn by the annular SDF in the ring fragment shader. */
export const buildRingDiscGeometry = (opts: RingOpts = {}): THREE.BufferGeometry => {
  const outerRadius = opts.outerRadius ?? 1.0;
  const segments = opts.segments ?? 64;
  // Slightly larger than outerRadius so SDF AA falloff isn't clipped.
  return new THREE.CircleGeometry(outerRadius * 1.05, segments);
};

export interface RingPickerOpts {
  radius: number;
  tube: number;
}

/** Torus aligned to the local XY plane (axis = local Z) — matches the disc's orientation. */
export const buildRingPickerGeometry = (opts: RingPickerOpts): THREE.BufferGeometry =>
  new THREE.TorusGeometry(opts.radius, opts.tube, 6, 32);

export interface PlaneHandleOpts {
  size?: number;
}

/** Square in local XY; material's SDF rounds the corners. */
export const buildPlaneHandleGeometry = (opts: PlaneHandleOpts = {}): THREE.BufferGeometry => {
  const size = opts.size ?? 0.22;
  return new THREE.PlaneGeometry(size, size);
};

export const buildPlaneHandlePickerGeometry = (opts: PlaneHandleOpts = {}): THREE.BufferGeometry =>
  new THREE.PlaneGeometry((opts.size ?? 0.22) * 1.05, (opts.size ?? 0.22) * 1.05);

/** Centre handle for uniform scale. */
export const buildUniformScaleGeometry = (radius = 0.06): THREE.BufferGeometry =>
  new THREE.OctahedronGeometry(radius, 1);

export const buildUniformScalePickerGeometry = (radius = 0.06): THREE.BufferGeometry =>
  new THREE.OctahedronGeometry(radius * 1.8, 1);
