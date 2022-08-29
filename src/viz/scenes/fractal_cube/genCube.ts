import * as THREE from 'three';

const Conf = {
  iterCount: 12,
  sizeMultiplierPerIter: 1.07,
  baseWidth: 80,
  baseThickness: 80,
};

/**
 * Given the provided `p1` and `p2` points, generates a `BoxGeometry` that stretches from `p1` to `p2`.
 *
 * `p1` and `p2` will be located at the midpoints of the sides of the cube.  The cube will be rotated to
 * face the direction of `p2` from `p1` with `thickness` as the thickness of the cube.
 */
const buildBoxBetweenPoints = (
  p1: THREE.Vector3,
  p2: THREE.Vector3,
  thickness1: number,
  thickness2: number = thickness1
): THREE.BoxGeometry => {
  const direction = p2.clone().sub(p1).normalize();
  const midpoint = p1.clone().add(p2).divideScalar(2);
  const length = p2.clone().sub(p1).length();
  const geometry = new THREE.BoxGeometry(thickness1, thickness2, length);

  // Box points along the Z axis by default (0, 0, 1)
  //
  // We want to rotate the cube to face the direction of `p2` from `p1`

  // Find the rotation axis
  const rotationAxis = new THREE.Vector3(0, 0, 1).cross(direction).normalize();
  // Find the rotation angle
  const rotationAngle = Math.acos(new THREE.Vector3(0, 0, 1).dot(direction));
  // Rotate the geometry
  const quat = new THREE.Quaternion().setFromAxisAngle(rotationAxis, rotationAngle);
  geometry.applyQuaternion(quat);

  geometry.translate(midpoint.x, midpoint.y, midpoint.z);
  return geometry;
};

const buildCubeIter = (iter: number, mat: THREE.Material): THREE.Group => {
  const group = new THREE.Group();
  group.name = `fractal_cube_${iter}`;

  const multiplier = Conf.sizeMultiplierPerIter ** iter;
  const width = Conf.baseWidth * multiplier;
  const thickness = Conf.baseThickness * multiplier;

  // Create a segment for each of the cube's 12 edges
  const edgeSegments = [
    {
      start: new THREE.Vector3(-width / 2, -width / 2, -thickness / 2),
      end: new THREE.Vector3(width / 2, -width / 2, -thickness / 2),
    },
    {
      start: new THREE.Vector3(width / 2, -width / 2, -thickness / 2),
      end: new THREE.Vector3(width / 2, width / 2, -thickness / 2),
    },
    {
      start: new THREE.Vector3(width / 2, width / 2, -thickness / 2),
      end: new THREE.Vector3(-width / 2, width / 2, -thickness / 2),
    },
    {
      start: new THREE.Vector3(-width / 2, width / 2, -thickness / 2),
      end: new THREE.Vector3(-width / 2, -width / 2, -thickness / 2),
    },
    {
      start: new THREE.Vector3(-width / 2, -width / 2, thickness / 2),
      end: new THREE.Vector3(width / 2, -width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(width / 2, -width / 2, thickness / 2),
      end: new THREE.Vector3(width / 2, width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(width / 2, width / 2, thickness / 2),
      end: new THREE.Vector3(-width / 2, width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(-width / 2, width / 2, thickness / 2),
      end: new THREE.Vector3(-width / 2, -width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(-width / 2, -width / 2, -thickness / 2),
      end: new THREE.Vector3(-width / 2, -width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(width / 2, -width / 2, -thickness / 2),
      end: new THREE.Vector3(width / 2, -width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(width / 2, width / 2, -thickness / 2),
      end: new THREE.Vector3(width / 2, width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(width / 2, width / 2, thickness / 2),
      end: new THREE.Vector3(width / 2, -width / 2, thickness / 2),
    },
    {
      start: new THREE.Vector3(-width / 2, width / 2, -thickness / 2),
      end: new THREE.Vector3(-width / 2, width / 2, thickness / 2),
    },
  ];

  for (let i = 0; i < edgeSegments.length; i++) {
    const edgeSegment = edgeSegments[i];
    const edgeGeom = buildBoxBetweenPoints(edgeSegment.start, edgeSegment.end, 5, 5);
    const edge = new THREE.Mesh(edgeGeom, mat);
    edge.name = `fractal_cube_${iter}_edge_${i}`;
    group.add(edge);
  }

  return group;
};

export const buildCube = (mat: THREE.Material): THREE.Group => {
  const group = new THREE.Group();
  group.name = 'big_fractal_cube';

  for (let i = 0; i < Conf.iterCount; i++) {
    const iter = buildCubeIter(i, mat);
    group.add(iter);
  }

  return group;
};
