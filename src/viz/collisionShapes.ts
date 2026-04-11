import * as THREE from 'three';

import type { AmmoInterface, BtCollisionShape, BtVec3 } from '../ammojs/ammoTypes';

export interface CollisionShapeBuildResult {
  shape: BtCollisionShape;
  /** Center offset in scaled local space — rotate by mesh quaternion and add to position */
  centerOffset?: THREE.Vector3;
  /** Local rotation of the detected shape, to be composed with mesh quaternion */
  localRotation?: THREE.Quaternion;
}

/**
 * If the build result has center/rotation offsets, composes them with the given
 * position and quaternion and returns adjusted copies.  Otherwise returns the
 * originals unmodified (no allocation).
 */
export const applyShapeBuildResult = (
  buildResult: CollisionShapeBuildResult,
  pos: THREE.Vector3,
  quat: THREE.Quaternion
): { pos: THREE.Vector3; quat: THREE.Quaternion } => {
  if (!buildResult.centerOffset && !buildResult.localRotation) {
    return { pos, quat };
  }
  const adjPos = pos.clone();
  const adjQuat = quat.clone();
  if (buildResult.centerOffset) {
    adjPos.add(buildResult.centerOffset.clone().applyQuaternion(adjQuat));
  }
  if (buildResult.localRotation) {
    adjQuat.multiply(buildResult.localRotation);
  }
  return { pos: adjPos, quat: adjQuat };
};

/**
 * Attempts to detect if a mesh's vertices form a rectangular box (including OBBs with
 * baked-in rotations).  GLTF-imported cubes typically have 24+ raw vertices but only
 * 8 unique positions — we deduplicate and check for a valid box.
 *
 * @param vertices  Raw position attribute array (local space, unscaled)
 * @param indices   Optional index array
 * @param scale     Scale to apply to vertices before detection
 * @returns Box parameters if detected, or null
 */
export const tryDetectBoxFromVertices = (
  vertices: Float32Array,
  indices: Uint16Array | undefined,
  scale: THREE.Vector3
): { halfExtents: THREE.Vector3; center: THREE.Vector3; quaternion: THREE.Quaternion } | null => {
  const EPS = 1e-4;

  // Deduplicate vertices by scaled position
  const unique: THREE.Vector3[] = [];
  const addUnique = (x: number, y: number, z: number) => {
    const sx = x * scale.x;
    const sy = y * scale.y;
    const sz = z * scale.z;
    for (const p of unique) {
      if (Math.abs(p.x - sx) < EPS && Math.abs(p.y - sy) < EPS && Math.abs(p.z - sz) < EPS) {
        return;
      }
    }
    unique.push(new THREE.Vector3(sx, sy, sz));
  };

  if (indices) {
    for (let i = 0; i < indices.length; i++) {
      const ix = indices[i] * 3;
      addUnique(vertices[ix], vertices[ix + 1], vertices[ix + 2]);
      if (unique.length > 8) return null;
    }
  } else {
    for (let i = 0; i < vertices.length; i += 3) {
      addUnique(vertices[i], vertices[i + 1], vertices[i + 2]);
      if (unique.length > 8) return null;
    }
  }

  if (unique.length !== 8) return null;

  // Try axis-aligned box first: check all 8 vertices are at AABB corners
  const min = new THREE.Vector3(Infinity, Infinity, Infinity);
  const max = new THREE.Vector3(-Infinity, -Infinity, -Infinity);
  for (const p of unique) {
    min.min(p);
    max.max(p);
  }

  let isAxisAligned = true;
  for (const p of unique) {
    const atCorner =
      (Math.abs(p.x - min.x) < EPS || Math.abs(p.x - max.x) < EPS) &&
      (Math.abs(p.y - min.y) < EPS || Math.abs(p.y - max.y) < EPS) &&
      (Math.abs(p.z - min.z) < EPS || Math.abs(p.z - max.z) < EPS);
    if (!atCorner) {
      isAxisAligned = false;
      break;
    }
  }

  if (isAxisAligned) {
    return {
      halfExtents: new THREE.Vector3().subVectors(max, min).multiplyScalar(0.5),
      center: new THREE.Vector3().addVectors(min, max).multiplyScalar(0.5),
      quaternion: new THREE.Quaternion(),
    };
  }

  // OBB detection: find 3 mutually orthogonal edge vectors from vertex 0.
  // A box corner has exactly 3 edge-adjacent neighbours; we test all C(7,3)=35
  // triplets to avoid assumptions about distance ordering.
  const v0 = unique[0];
  const others = unique.slice(1); // 7 vertices
  const edges: THREE.Vector3[] = others.map(v => new THREE.Vector3().subVectors(v, v0));

  for (let i = 0; i < 7; i++) {
    for (let j = i + 1; j < 7; j++) {
      for (let k = j + 1; k < 7; k++) {
        const e1 = edges[i];
        const e2 = edges[j];
        const e3 = edges[k];

        // Check orthogonality using cosine threshold
        const l1 = e1.length(),
          l2 = e2.length(),
          l3 = e3.length();
        if (l1 < EPS || l2 < EPS || l3 < EPS) continue;

        if (Math.abs(e1.dot(e2)) / (l1 * l2) > EPS) continue;
        if (Math.abs(e1.dot(e3)) / (l1 * l3) > EPS) continue;
        if (Math.abs(e2.dot(e3)) / (l2 * l3) > EPS) continue;

        // Verify the other 4 vertices sit at the expected box corners
        const expectedCorners = [
          new THREE.Vector3().copy(v0).add(e1).add(e2),
          new THREE.Vector3().copy(v0).add(e1).add(e3),
          new THREE.Vector3().copy(v0).add(e2).add(e3),
          new THREE.Vector3().copy(v0).add(e1).add(e2).add(e3),
        ];

        let allFound = true;
        for (const expected of expectedCorners) {
          let found = false;
          for (const p of unique) {
            if (p.distanceTo(expected) < EPS * 100) {
              found = true;
              break;
            }
          }
          if (!found) {
            allFound = false;
            break;
          }
        }
        if (!allFound) continue;

        // Valid box found — build rotation from the 3 edge axes
        const halfExtents = new THREE.Vector3(l1 / 2, l2 / 2, l3 / 2);
        const obbCenter = new THREE.Vector3()
          .copy(v0)
          .addScaledVector(e1, 0.5)
          .addScaledVector(e2, 0.5)
          .addScaledVector(e3, 0.5);

        const ax = e1.clone().normalize();
        const ay = e2.clone().normalize();
        const az = e3.clone().normalize();

        // Ensure right-handed basis
        if (new THREE.Vector3().crossVectors(ax, ay).dot(az) < 0) {
          az.negate();
        }

        const rotMatrix = new THREE.Matrix4().makeBasis(ax, ay, az);
        const quaternion = new THREE.Quaternion().setFromRotationMatrix(rotMatrix);

        return { halfExtents, center: obbCenter, quaternion };
      }
    }
  }

  return null;
};

export const buildTrimeshShape = (
  Ammo: AmmoInterface,
  indices: Uint16Array | undefined,
  vertices: Float32Array,
  scale: THREE.Vector3 = new THREE.Vector3(1, 1, 1)
) => {
  const numVertices = vertices.length / 3;
  const numTriangles = indices ? indices.length / 3 : numVertices / 3;

  // Write scaled vertex positions into an Ammo heap buffer (float32, 12-byte stride).
  // `btTriangleIndexVertexArrayWrapper` holds a raw pointer and must not be freed while the shape lives.
  const vertexPtr = Ammo._malloc(numVertices * 3 * 4);
  const vertexHeap = new Float32Array(Ammo.HEAPF32.buffer, vertexPtr, numVertices * 3);
  if (scale.x === 1 && scale.y === 1 && scale.z === 1) {
    vertexHeap.set(vertices);
  } else {
    for (let i = 0; i < vertices.length; i += 3) {
      vertexHeap[i] = vertices[i] * scale.x;
      vertexHeap[i + 1] = vertices[i + 1] * scale.y;
      vertexHeap[i + 2] = vertices[i + 2] * scale.z;
    }
  }

  // Write int32 indices (`btTriangleIndexVertexArrayWrapper` defaults to `PHY_INTEGER`).
  const numIndexInts = numTriangles * 3;
  const indexPtr = Ammo._malloc(numIndexInts * 4);
  const indexHeap = new Int32Array(Ammo.HEAPF32.buffer, indexPtr, numIndexInts);
  if (indices) {
    for (let i = 0; i < indices.length; i++) {
      indexHeap[i] = indices[i];
    }
  } else {
    for (let i = 0; i < numIndexInts; i++) {
      indexHeap[i] = i;
    }
  }

  const indexedArray = new Ammo.btTriangleIndexVertexArrayWrapper(
    numTriangles,
    indexPtr,
    3 * 4, // triangle index stride: 3 × int32
    numVertices,
    vertexPtr,
    3 * 4 // vertex stride: 3 × float32
  );
  return new Ammo.btBvhTriangleMeshShape(indexedArray, true, true);
};

export const buildConvexHullShape = (
  Ammo: AmmoInterface,
  btvec3: (x: number, y: number, z: number) => BtVec3,
  indices: Uint16Array | undefined,
  vertices: Float32Array,
  scale: THREE.Vector3 = new THREE.Vector3(1, 1, 1)
) => {
  const hull = new Ammo.btConvexHullShape();

  if (indices) {
    for (let i = 0; i < indices.length; i++) {
      const ix = indices[i] * 3;
      hull.addPoint(btvec3(vertices[ix] * scale.x, vertices[ix + 1] * scale.y, vertices[ix + 2] * scale.z));
    }
  } else {
    for (let i = 0; i < vertices.length; i += 3) {
      hull.addPoint(btvec3(vertices[i] * scale.x, vertices[i + 1] * scale.y, vertices[i + 2] * scale.z));
    }
  }

  return hull;
};

export const buildCollisionShapeFromMesh = (
  Ammo: AmmoInterface,
  btvec3: (x: number, y: number, z: number) => BtVec3,
  mesh: THREE.Mesh,
  extraScale?: THREE.Vector3
): CollisionShapeBuildResult => {
  if (mesh.geometry instanceof THREE.BoxGeometry) {
    const halfExtents = btvec3(
      mesh.geometry.parameters.width * mesh.scale.x * (extraScale?.x ?? 1) * 0.5,
      mesh.geometry.parameters.height * mesh.scale.y * (extraScale?.y ?? 1) * 0.5,
      mesh.geometry.parameters.depth * mesh.scale.z * (extraScale?.z ?? 1) * 0.5
    );
    return { shape: new Ammo.btBoxShape(halfExtents) };
  } else if (
    (mesh.geometry instanceof THREE.SphereGeometry ||
      (mesh.geometry instanceof THREE.IcosahedronGeometry && mesh.geometry.parameters.detail >= 2)) &&
    mesh.scale.x === mesh.scale.y &&
    mesh.scale.y === mesh.scale.z &&
    (!extraScale || (extraScale.x === extraScale.y && extraScale.y === extraScale.z))
  ) {
    const radius = mesh.geometry.parameters.radius * mesh.scale.x * (extraScale?.x ?? 1);
    return { shape: new Ammo.btSphereShape(radius) };
  }

  const geometry = mesh.geometry as THREE.BufferGeometry;
  const vertices = geometry.attributes.position.array as Float32Array | Uint16Array;
  const indices = geometry.index?.array as Uint16Array | undefined;
  if (vertices instanceof Uint16Array) {
    throw new Error('GLTF Quantization not yet supported');
  }
  let scale = mesh.scale.clone();
  if (extraScale) {
    scale = scale.multiply(extraScale);
  }

  // Detect boxes from raw vertex data (catches GLTF-imported cubes, scaled boxes, and
  // boxes with baked-in rotations that don't use THREE.BoxGeometry)
  if (!mesh.userData.convexhull && !mesh.userData.convexHull) {
    const boxResult = tryDetectBoxFromVertices(vertices, indices, scale);
    if (boxResult) {
      const shape = new Ammo.btBoxShape(
        btvec3(boxResult.halfExtents.x, boxResult.halfExtents.y, boxResult.halfExtents.z)
      );
      const isIdentityQuat =
        Math.abs(boxResult.quaternion.x) < 1e-6 &&
        Math.abs(boxResult.quaternion.y) < 1e-6 &&
        Math.abs(boxResult.quaternion.z) < 1e-6 &&
        Math.abs(boxResult.quaternion.w - 1) < 1e-6;
      const hasCenterOffset = boxResult.center.lengthSq() > 1e-8;
      return {
        shape,
        centerOffset: hasCenterOffset ? boxResult.center : undefined,
        localRotation: isIdentityQuat ? undefined : boxResult.quaternion,
      };
    }
  }

  if (mesh.userData.convexhull || mesh.userData.convexHull) {
    return { shape: buildConvexHullShape(Ammo, btvec3, indices, vertices, scale) };
  }
  return { shape: buildTrimeshShape(Ammo, indices, vertices, scale) };
};
