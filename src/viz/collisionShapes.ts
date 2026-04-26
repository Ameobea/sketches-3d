import * as THREE from 'three';

import type {
  AmmoInterface,
  BtBvhTriangleMeshShape,
  BtCollisionShape,
  BtTriangleInfoMap,
  BtVec3,
  btTriangleIndexVertexArrayWrapper,
} from '../ammojs/ammoTypes';

export interface CollisionShapeBuildResult {
  shape: BtCollisionShape;
  /** Releases the shape and any additional Wasm resources it owns. */
  destroyShape?: (Ammo: AmmoInterface) => void;
  /** Center offset in scaled local space — rotate by mesh quaternion and add to position */
  centerOffset?: THREE.Vector3;
  /** Local rotation of the detected shape, to be composed with mesh quaternion */
  localRotation?: THREE.Quaternion;
}

const shouldDebugInternalEdges = () =>
  typeof window !== 'undefined' &&
  (((window as any).__dreamInternalEdgeDebug as boolean | undefined) === true ||
    new URLSearchParams(window.location.search).get('debugInternalEdges') === '1');

const generateInternalEdgeInfo = (
  Ammo: AmmoInterface,
  meshShape: BtBvhTriangleMeshShape,
  triangleInfoMap: BtTriangleInfoMap
) => {
  const generate =
    Ammo.btInternalEdgeUtility.btGenerateInternalEdgeInfo ??
    Ammo.btInternalEdgeUtility.prototype.btGenerateInternalEdgeInfo;
  generate(meshShape, triangleInfoMap);
};

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
  indices: Uint16Array | Uint32Array | undefined,
  vertices: Float32Array,
  scale: THREE.Vector3 = new THREE.Vector3(1, 1, 1)
): CollisionShapeBuildResult => {
  const numVertices = vertices.length / 3;
  const numTriangles = indices ? indices.length / 3 : numVertices / 3;

  let vertexPtr = 0;
  let indexPtr = 0;
  let indexedArray: btTriangleIndexVertexArrayWrapper | null = null;
  let meshShape: BtBvhTriangleMeshShape | null = null;
  let triangleInfoMap: BtTriangleInfoMap | null = null;

  try {
    // Write scaled vertex positions into an Ammo heap buffer (float32, 12-byte stride).
    // `btTriangleIndexVertexArrayWrapper` holds a raw pointer and must not be freed while the shape lives.
    vertexPtr = Ammo._malloc(numVertices * 3 * 4);
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
    indexPtr = Ammo._malloc(numIndexInts * 4);
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

    indexedArray = new Ammo.btTriangleIndexVertexArrayWrapper(
      numTriangles,
      indexPtr,
      3 * 4, // triangle index stride: 3 × int32
      numVertices,
      vertexPtr,
      3 * 4 // vertex stride: 3 × float32
    );
    meshShape = new Ammo.btBvhTriangleMeshShape(indexedArray, true, true);

    triangleInfoMap = new Ammo.btTriangleInfoMap();
    generateInternalEdgeInfo(Ammo, meshShape, triangleInfoMap);
    meshShape.setTriangleInfoMap(triangleInfoMap);

    return {
      shape: meshShape,
      destroyShape: Ammo => {
        Ammo.destroy(meshShape!);
        Ammo.destroy(triangleInfoMap!);
        Ammo.destroy(indexedArray!);
        Ammo._free(indexPtr);
        Ammo._free(vertexPtr);
      },
    };
  } catch (err) {
    if (meshShape) {
      Ammo.destroy(meshShape);
    }
    if (triangleInfoMap) {
      Ammo.destroy(triangleInfoMap);
    }
    if (indexedArray) {
      Ammo.destroy(indexedArray);
    }
    if (indexPtr !== 0) {
      Ammo._free(indexPtr);
    }
    if (vertexPtr !== 0) {
      Ammo._free(vertexPtr);
    }
    throw err;
  }
};

/**
 * Precomputed collision geometry override for an asset.  When passed to
 * `buildCollisionShapeFromMesh`, it bypasses the visual mesh's own geometry and the
 * box/sphere detection path entirely — the resulting shape is a `btBvhTriangleMeshShape`
 * over the override verts/indices, scaled per-instance from `mesh.scale * extraScale`.
 *
 * Used for `colliderShape: 'convexHull'` assets: a real convex hull mesh is computed
 * once per asset (via Manifold) and the same hull data is reused across every instance.
 */
export interface CollisionMeshOverride {
  verts: Float32Array;
  indices: Uint16Array | Uint32Array | undefined;
}

/**
 * Extract the vertex set that should feed into a convex-hull computation for `geometry`.
 *
 * For indexed geometries we iterate the index buffer and emit only referenced verts —
 * this matches the pre-refactor `btConvexHullShape` behavior, where unused/orphan verts
 * in the position buffer never expanded the hull.  For non-indexed geometries we honor
 * the attribute's `count` field in case the underlying buffer is over-allocated.
 *
 * The output is a fresh Float32Array (xyz-packed) safe to transfer to a worker.
 *
 * Throws if the position attribute isn't a Float32Array (e.g. GLTF-quantized geometry).
 */
export const extractHullInputVertices = (geometry: THREE.BufferGeometry): Float32Array => {
  const positionAttr = geometry.attributes.position;
  if (!(positionAttr.array instanceof Float32Array)) {
    throw new Error(
      `extractHullInputVertices: position attribute is not a Float32Array (got ${positionAttr.array.constructor.name}); GLTF quantization is not supported for convex-hull assets`
    );
  }
  const verts = positionAttr.array;
  const indexAttr = geometry.index;
  if (indexAttr) {
    const indices = indexAttr.array;
    const out = new Float32Array(indices.length * 3);
    for (let i = 0; i < indices.length; i++) {
      const ix = indices[i] * 3;
      out[i * 3] = verts[ix];
      out[i * 3 + 1] = verts[ix + 1];
      out[i * 3 + 2] = verts[ix + 2];
    }
    return out;
  }
  return verts.slice(0, positionAttr.count * 3);
};

/** Build the trimesh shape and (when enabled) emit the internal-edge debug log. */
const buildTrimeshShapeWithDebug = (
  Ammo: AmmoInterface,
  indices: Uint16Array | Uint32Array | undefined,
  verts: Float32Array,
  scale: THREE.Vector3,
  meshName: string,
  source: 'override' | 'mesh'
): CollisionShapeBuildResult => {
  const buildResult = buildTrimeshShape(Ammo, indices, verts, scale);
  if (shouldDebugInternalEdges()) {
    const meshShape = buildResult.shape as BtBvhTriangleMeshShape;
    const infoMap = meshShape.getTriangleInfoMap();
    const tag =
      source === 'override'
        ? '[internal-edge] attached triangle info map (override)'
        : '[internal-edge] attached triangle info map';
    console.info(tag, {
      meshName: meshName || '<unnamed>',
      shapePtr: Ammo.getPointer(meshShape),
      infoMapPtr: Ammo.getPointer(infoMap),
      triangleCount: indices ? indices.length / 3 : verts.length / 9,
    });
  }
  return buildResult;
};

export const buildCollisionShapeFromMesh = (
  Ammo: AmmoInterface,
  btvec3: (x: number, y: number, z: number) => BtVec3,
  mesh: THREE.Mesh,
  extraScale?: THREE.Vector3,
  collisionMeshOverride?: CollisionMeshOverride
): CollisionShapeBuildResult => {
  let scale = mesh.scale.clone();
  if (extraScale) {
    scale = scale.multiply(extraScale);
  }

  if (collisionMeshOverride) {
    return buildTrimeshShapeWithDebug(
      Ammo,
      collisionMeshOverride.indices,
      collisionMeshOverride.verts,
      scale,
      mesh.name,
      'override'
    );
  }

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

  // Detect boxes from raw vertex data (catches GLTF-imported cubes, scaled boxes, and
  // boxes with baked-in rotations that don't use THREE.BoxGeometry)
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

  return buildTrimeshShapeWithDebug(Ammo, indices, vertices, scale, mesh.name, 'mesh');
};
