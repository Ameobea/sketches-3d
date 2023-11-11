/**
 * Adapted from: https://mathematica.stackexchange.com/questions/77165/3d-vicsek-fractal-notebook
 */

import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { buildCustomShader } from 'src/viz/shaders/customShader';

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

const buildAndAdd3DVicsekFractal = (
  viz: VizState,
  pos: THREE.Vector3,
  scale: number,
  iterations: number,
  material: THREE.Material,
  mutatePositionsCb?: (positions: Point3D[]) => Point3D[],
  collide = true,
  name?: string
) => {
  const stage = viz.scene;
  let positions = generate3DVicsekFractal([[pos.x, pos.y, pos.z]], scale, iterations);
  if (mutatePositionsCb) {
    positions = mutatePositionsCb(positions);
  }

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
  }

  if (collide) {
    viz.collisionWorldLoadedCbs.push(fpCtx => {
      for (const pos of positions) {
        // if it's below the kill floor or too high to reach, no need to worry about colliding with it
        if (pos[1] < -120 || pos[1] > 50) {
          continue;
        }

        fpCtx.addBox(pos, [finalScale / 2, finalScale / 2, finalScale / 2]);
      }
      return;

      // \/ This turns out to be massively slower for some reason

      // compute average position of all cubes to serve as the origin of the compound shape
      const avgPos = positions
        .reduce(
          (acc, pos) => {
            acc[0] += pos[0];
            acc[1] += pos[1];
            acc[2] += pos[2];
            return acc;
          },
          [0, 0, 0]
        )
        .map(v => v / positions.length) as Point3D;

      const halfExtents = [finalScale / 2, finalScale / 2, finalScale / 2] as Point3D;
      fpCtx.addCompound(
        avgPos,
        positions.map(pos => {
          // Transform pos to be relative to the origin
          const relativePos = pos.map((v, i) => v - avgPos[i]) as Point3D;

          return { type: 'box', pos: relativePos, halfExtents };
        })
      );
    });
  }

  mesh.name = name ?? `3DVicsekFractal_${pos.x},${pos.y},${pos.z}_${scale}_${iterations}`;

  stage.add(mesh);
};

export const buildAndAddFractals = (
  viz: VizState,
  cubesTexture: THREE.Texture,
  cubesTextureNormal: THREE.Texture,
  cubesTextureRoughness: THREE.Texture
) => {
  const cubesMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      map: cubesTexture,
      normalMap: cubesTextureNormal,
      roughnessMap: cubesTextureRoughness,
      metalness: 0.7,
      roughness: 0.4,
      uvTransform: new THREE.Matrix3().scale(0.21, 0.21),
      mapDisableDistance: 300,
      normalScale: 1.2,
      ambientLightScale: 1.3,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
    }
  );

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(182, -7, 180.5), 160, 4, cubesMaterial, positions =>
    positions.filter(pos => {
      if (pos[1] > 50) {
        return true;
      }
      if (pos[1] < -50) {
        return true;
      }
      if (pos[0] > 194) {
        return false;
      }

      if (pos[0] < 0 && pos[2] > 140) {
        return false;
      }
      if (pos[0] < 30 && pos[2] > 200) {
        return false;
      }
      if (pos[0] > 175 && pos[2] > 260) {
        return false;
      }
      return true;
    })
  );

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(28, 120, 42), 80, 3, cubesMaterial, undefined, true);

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(-110, 80, 118), 80, 3, cubesMaterial, undefined, true);

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(50, 50, -30), 80 / 3, 2, cubesMaterial, undefined, false);
  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(50, -10, -20), 80 / 3, 2, cubesMaterial, undefined, true);

  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(-45, 9, -34),
    80 / 3 / 2,
    2,
    cubesMaterial,
    undefined,
    true
  );
  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(-15, 74, 44), 80 / 3, 2, cubesMaterial, undefined, true);
  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(100, 64, 114), 80 / 3, 2, cubesMaterial, undefined, true);

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(300, 100, 30), 180, 4, cubesMaterial, undefined, true);
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(225, 58.5, 110),
    180 / 3,
    3,
    // new THREE.MeshBasicMaterial({ color: new THREE.Color(0xffffff) }),
    cubesMaterial,
    undefined,
    true
  );
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(250, 60, 170),
    180 / 3,
    3,
    // new THREE.MeshBasicMaterial({ color: new THREE.Color(0xffffff) }),
    cubesMaterial,
    undefined,
    true
  );

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(140, -280, 250), 180, 4, cubesMaterial, undefined, true);
  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(40, 280, 250), 180, 4, cubesMaterial, undefined, true);

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(140, 40, -250), 180, 4, cubesMaterial, undefined, true);

  buildAndAdd3DVicsekFractal(viz, new THREE.Vector3(100, 40, 450), 180, 4, cubesMaterial, undefined, true);
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(-150, 37, 250),
    180,
    4,
    cubesMaterial,
    // new THREE.MeshBasicMaterial({ color: new THREE.Color(0xffffff) }),
    undefined,
    true
  );
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(-70, -40, 180),
    180 / 3,
    3,
    cubesMaterial,
    positions => positions.filter(pos => pos[2] < 200 && !(pos[0] < -90 && pos[2] > 170)),
    true
  );
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(-40, -40, 350),
    180 / 3,
    3,
    cubesMaterial,
    undefined,
    false
  );
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(-90, 10, -2),
    140 / 3 / 3,
    2,
    cubesMaterial,
    undefined,
    true
  );
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(-90, -200, 90),
    180,
    4,
    // new THREE.MeshBasicMaterial({ color: new THREE.Color(0xffffff) }),
    cubesMaterial,
    undefined,
    true
  );
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(300, 120, 400),
    280,
    4,
    // new THREE.MeshBasicMaterial({ color: new THREE.Color(0xffffff) }),
    cubesMaterial,
    undefined,
    true
  );
  buildAndAdd3DVicsekFractal(
    viz,
    new THREE.Vector3(30, -50, -90),
    180,
    4,
    // new THREE.MeshBasicMaterial({ color: new THREE.Color(0xffffff) }),
    cubesMaterial,
    undefined,
    true
  );
};
