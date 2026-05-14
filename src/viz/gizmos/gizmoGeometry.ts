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
  const radius = opts.radius ?? 0.013;
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

export interface TaperedShaftOpts extends ShaftOpts {
  /** Length of the cone-tip taper at +Y; defaults to 5% of `length`. */
  taperLength?: number;
}

/** Cylinder that tapers to a point at +Y so it can extend into an arrowhead without poking past its silhouette. */
export const buildTaperedShaftGeometry = (opts: TaperedShaftOpts = {}): THREE.BufferGeometry => {
  const length = opts.length ?? 1.0;
  const radius = opts.radius ?? 0.013;
  const segments = opts.segments ?? 12;
  const taperLength = Math.max(0, Math.min(opts.taperLength ?? length * 0.05, length));
  const cylinderEnd = length - taperLength;

  // Two rings (base, shoulder) + single tip vertex; indices fan the taper into the tip.
  const positions = new Float32Array((segments * 2 + 1) * 3);
  for (let r = 0; r < 2; r++) {
    const y = r === 0 ? 0 : cylinderEnd;
    const base = r * segments;
    for (let i = 0; i < segments; i++) {
      const theta = (i / segments) * Math.PI * 2;
      positions[(base + i) * 3 + 0] = radius * Math.cos(theta);
      positions[(base + i) * 3 + 1] = y;
      positions[(base + i) * 3 + 2] = radius * Math.sin(theta);
    }
  }
  const tipIdx = 2 * segments;
  positions[tipIdx * 3 + 0] = 0;
  positions[tipIdx * 3 + 1] = length;
  positions[tipIdx * 3 + 2] = 0;

  const indices: number[] = [];
  for (let i = 0; i < segments; i++) {
    const i0 = i;
    const i1 = (i + 1) % segments;
    const j0 = segments + i;
    const j1 = segments + ((i + 1) % segments);
    indices.push(i0, i1, j1, i0, j1, j0);
  }
  for (let i = 0; i < segments; i++) {
    const j0 = segments + i;
    const j1 = segments + ((i + 1) % segments);
    indices.push(j0, j1, tipIdx);
  }

  const geo = new THREE.BufferGeometry();
  geo.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  geo.setIndex(indices);
  geo.computeVertexNormals();
  return geo;
};

export interface ArrowheadOpts {
  length?: number;
  radius?: number;
  radialSegments?: number;
}

export const buildArrowheadGeometry = (opts: ArrowheadOpts = {}): THREE.BufferGeometry => {
  const length = opts.length ?? 0.18;
  const radius = opts.radius ?? 0.06;
  // Bounding-box quad; buildArrowheadMaterial billboards it and draws the triangle via SDF.
  const geo = new THREE.BufferGeometry();
  const verts = new Float32Array([-radius, 0, 0, radius, 0, 0, radius, length, 0, -radius, length, 0]);
  geo.setAttribute('position', new THREE.BufferAttribute(verts, 3));
  geo.setIndex([0, 1, 2, 0, 2, 3]);
  return geo;
};

export const buildArrowheadPickerGeometry = (opts: ArrowheadOpts = {}): THREE.BufferGeometry => {
  // Real 3D cone — the raycaster needs volume even when the visual is billboarded.
  const length = opts.length ?? 0.22;
  const radius = (opts.radius ?? 0.06) * 1.35;
  const radialSegments = opts.radialSegments ?? 8;
  const geo = new THREE.ConeGeometry(radius, length, radialSegments, 1, false);
  geo.translate(0, length / 2, 0);
  return geo;
};

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

/** Centre handle for uniform scale.  `detail=0` so faces stay flat for the edgeOutline shader. */
export const buildUniformScaleGeometry = (radius = 0.06): THREE.BufferGeometry =>
  new THREE.OctahedronGeometry(radius, 0);

export const buildUniformScalePickerGeometry = (radius = 0.06): THREE.BufferGeometry =>
  new THREE.OctahedronGeometry(radius * 1.8, 0);
