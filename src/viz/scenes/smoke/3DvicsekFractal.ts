/**
 * Adapted from: https://mathematica.stackexchange.com/questions/77165/3d-vicsek-fractal-notebook
 */

import * as THREE from 'three';

import type { VizState } from 'src/viz';

type Point3D = [number, number, number];
type Cube = { position: Point3D; scale: number };

const p: Point3D[] = [
  [-1, 0, 0],
  [0, -1, 0],
  [0, 0, -1],
  [0, 0, 0],
  [0, 0, 1],
  [0, 1, 0],
  [1, 0, 0],
];

const translateAndScale = (pointA: Point3D, pointB: Point3D, scale: number): Point3D => [
  pointA[0] + pointB[0] * scale,
  pointA[1] + pointB[1] * scale,
  pointA[2] + pointB[2] * scale,
];

// f[x_] := Scale[Translate[x, p], 1/3]

// Graphics3D[Nest[f, Cuboid[], 3], Boxed -> False]

const generate3DVicsekFractal = (positions: Point3D[], scale: number, iterations: number): Point3D[] => {
  if (iterations === 0) {
    return positions;
  }

  const newPositions: Point3D[] = [];
  for (const position of positions) {
    for (const point of p) {
      newPositions.push(translateAndScale(position, point, scale));
    }
  }

  return generate3DVicsekFractal(newPositions, scale / 3, iterations - 1);
};

const computeFinalScale = (scale: number, iterations: number): number => {
  let finalScale = scale * 3;
  for (let i = 0; i < iterations; i += 1) {
    finalScale /= 3;
  }
  return finalScale;
};

export const buildAndAdd3DVicsekFractal = (
  viz: VizState,
  pos: THREE.Vector3,
  scale: number,
  iterations: number,
  material: THREE.Material,
  mutatePositionsCb?: (positions: Point3D[]) => Point3D[]
) => {
  const stage = viz.scene;
  let positions = generate3DVicsekFractal([[pos.x, pos.y, pos.z]], scale, iterations);
  if (mutatePositionsCb) {
    positions = mutatePositionsCb(positions);
  }

  // Render with instanced mesh
  const geometry = new THREE.BoxGeometry(1, 1, 1);
  const mesh = new THREE.InstancedMesh(geometry, material, positions.length);
  mesh.receiveShadow = true;
  mesh.castShadow = true;
  const matrix = new THREE.Matrix4();
  const finalScale = computeFinalScale(scale, iterations);
  matrix.scale(new THREE.Vector3(finalScale, finalScale, finalScale));
  for (let i = 0; i < positions.length; i++) {
    const pos = positions[i];
    matrix.setPosition(pos[0], pos[1], pos[2]);
    mesh.setMatrixAt(i, matrix);
    viz.collisionWorldLoadedCbs.push(fpCtx =>
      fpCtx.addBox(pos, [finalScale / 2, finalScale / 2, finalScale / 2])
    );
  }

  stage.add(mesh);
};
