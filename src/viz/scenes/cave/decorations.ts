import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { CustomBasicShaderMaterial } from 'src/viz/shaders/customBasicShader';

export const addDecorations = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  stalagMaterial: CustomBasicShaderMaterial
) => {
  const engine = await import('../../wasmComp/cave');
  const { memory } = await engine.default();

  const caveMesh = loadedWorld.getObjectByName('cave') as THREE.Mesh;
  const nonIndexedGeometry = caveMesh.geometry.index ? caveMesh.geometry.toNonIndexed() : caveMesh.geometry;
  // if vertices are indexed, we need to convert them to non-indexed
  const verts = nonIndexedGeometry.attributes.position.array;
  if (!(verts instanceof Float32Array)) {
    throw new Error('Expected vertices to be Float32Array');
  }
  const normals = nonIndexedGeometry.attributes.normal.array;
  if (!(normals instanceof Float32Array)) {
    throw new Error('Expected normals to be Float32Array');
  }
  caveMesh.updateMatrixWorld();
  // column-major order
  const transformArray = new Float32Array(16);
  caveMesh.matrixWorld.toArray(transformArray);
  engine.compute_stalags(verts, normals, transformArray);

  const stalagCount = engine.stalag_count();
  const f32Mem = new Float32Array(memory.buffer);

  const stalagPositionsPtr = engine.get_stalag_positions();
  const stalagPositions = f32Mem.subarray(
    stalagPositionsPtr / Float32Array.BYTES_PER_ELEMENT,
    stalagPositionsPtr / Float32Array.BYTES_PER_ELEMENT + stalagCount * 3
  );

  const stalagScalesPtr = engine.get_stalag_scales();
  const stalagScales = f32Mem.subarray(
    stalagScalesPtr / Float32Array.BYTES_PER_ELEMENT,
    stalagScalesPtr / Float32Array.BYTES_PER_ELEMENT + stalagCount * 3
  );

  const stalagEulerRotationsPtr = engine.get_stalag_euler_angles();
  const stalagEulerRotations = f32Mem.subarray(
    stalagEulerRotationsPtr / Float32Array.BYTES_PER_ELEMENT,
    stalagEulerRotationsPtr / Float32Array.BYTES_PER_ELEMENT + stalagCount * 3
  );

  const StalagBaseRadius = 0.2;
  const StalagBaseHeight = 7.5;
  const stalagBase = loadedWorld.getObjectByName('stalag') as THREE.Mesh;
  const stalagGeom = stalagBase.geometry.toNonIndexed();
  const stalags = new THREE.InstancedMesh(stalagGeom, stalagMaterial, stalagCount);
  stalags.instanceMatrix.setUsage(THREE.StaticDrawUsage);
  stalags.castShadow = false;
  stalags.receiveShadow = false;

  const matrix = new THREE.Matrix4();
  const euler = new THREE.Euler();
  const scale = new THREE.Vector3();
  for (let i = 0; i < stalagCount; i++) {
    euler.set(stalagEulerRotations[i * 3], stalagEulerRotations[i * 3 + 1], stalagEulerRotations[i * 3 + 2]);
    matrix.makeRotationFromEuler(euler);
    matrix.setPosition(stalagPositions[i * 3], stalagPositions[i * 3 + 1], stalagPositions[i * 3 + 2]);
    scale.set(stalagScales[i * 3], stalagScales[i * 3 + 1], stalagScales[i * 3 + 2]);
    matrix.scale(scale);
    stalags.setMatrixAt(i, matrix);
  }
  stalags.instanceMatrix.needsUpdate = true;

  const pos = new THREE.Vector3();
  const quat = new THREE.Quaternion().identity();
  viz.collisionWorldLoadedCbs.push(fpCtx => {
    for (let i = 0; i < stalagCount; i++) {
      stalags.getMatrixAt(i, matrix);
      pos.set(stalagPositions[i * 3], stalagPositions[i * 3 + 1], stalagPositions[i * 3 + 2]);
      scale.setFromMatrixScale(matrix);
      const radius = Math.max(scale.x, scale.z) * StalagBaseRadius;
      const height = scale.y * StalagBaseHeight;
      euler.set(
        stalagEulerRotations[i * 3],
        stalagEulerRotations[i * 3 + 1],
        stalagEulerRotations[i * 3 + 2]
      );
      quat.setFromEuler(euler);

      fpCtx.addCone(pos, radius, height, quat);
    }
  });

  viz.scene.add(stalags);
};
