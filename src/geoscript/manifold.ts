import ManifoldModule, { type Manifold, type ManifoldToplevel, type Mat4, type Vec3 } from 'manifold-3d';
import manifoldWasmURL from 'manifold-3d/manifold.wasm?url';

// import ManifoldModule from './manifoldComp/manifold';
// import type { Manifold, ManifoldToplevel, Vec3 } from 'manifold-3d';
// import manifoldWasURL from './manifoldComp/manifold.wasm?url';

let ManifoldWasm: ManifoldToplevel | null = null;

export const initManifoldWasm = async () => {
  if (ManifoldWasm) {
    return ManifoldWasm;
  }

  ManifoldWasm = await ManifoldModule({ locateFile: () => manifoldWasmURL });
  ManifoldWasm.setup();
  return ManifoldWasm;
};

enum BooleanOperation {
  Union = 0,
  Intersection = 1,
  Difference = 2,
}

let curHandle = 0;
const getNewHandle = () => {
  curHandle += 1;
  return curHandle;
};

const MeshHandles: Map<number, Manifold> = new Map();

export const drop_mesh_handle = (handle: number) => {
  const mesh = MeshHandles.get(handle);
  if (!mesh) {
    console.warn(`No mesh found for handle ${handle}`);
    return;
  }

  mesh.delete();
  MeshHandles.delete(handle);
};

export const drop_all_mesh_handles = () => {
  MeshHandles.forEach(mesh => mesh.delete());
  MeshHandles.clear();
};

const encodeManifoldMesh = (manifold: Manifold, handleOnly: boolean) => {
  const handle = getNewHandle();
  MeshHandles.set(handle, manifold);

  if (handleOnly) {
    const buffer = new ArrayBuffer(Uint32Array.BYTES_PER_ELEMENT * 4);
    const u32View = new Uint32Array(buffer);
    u32View[2] = handle;
    return new Uint8Array(buffer);
  }

  const outMesh = manifold.getMesh();
  const outVerts = outMesh.vertProperties.slice();
  const outIndices = outMesh.triVerts;
  // let v3 = new THREE.Vector3();
  // console.log(outMesh.runOriginalID);
  // console.log(outMesh.runIndex, outMesh.numRun);
  // console.log(outMesh.triVerts);
  // console.log(outMesh.runTransform);
  // if (outMesh.mergeFromVert.length > 0 || outMesh.mergeToVert.length > 0) {
  //   throw new Error('unimplemented');
  // }
  // const transform = new THREE.Matrix4();
  // for (let runIx = 0; runIx < outMesh.numRun; runIx += 1) {
  //   const start = outMesh.runIndex[runIx];
  //   const end = outMesh.runIndex[runIx + 1];

  //   const rawTransform = outMesh.transform(runIx);

  //   transform.set(
  //     rawTransform[0],
  //     rawTransform[4],
  //     rawTransform[8],
  //     rawTransform[12],
  //     rawTransform[1],
  //     rawTransform[5],
  //     rawTransform[9],
  //     rawTransform[13],
  //     rawTransform[2],
  //     rawTransform[6],
  //     rawTransform[10],
  //     rawTransform[14],
  //     0,
  //     0,
  //     0,
  //     1
  //   );

  //   for (let vtxIx = start; vtxIx < end; vtxIx += 3) {
  //     v3.set(
  //       outMesh.vertProperties[outIndices[vtxIx]],
  //       outMesh.vertProperties[outIndices[vtxIx + 1]],
  //       outMesh.vertProperties[outIndices[vtxIx + 2]]
  //     );
  //     // v3 = v3.applyMatrix4(transform);
  //     outVerts[outIndices[vtxIx]] = v3.x;
  //     outVerts[outIndices[vtxIx + 1]] = v3.y;
  //     outVerts[outIndices[vtxIx + 2]] = v3.z;
  //   }
  // }

  const vtxCount = outVerts.length / 3;
  const triangleCount = outIndices.length / 3;

  // encode the output mesh into binary with the following structure:
  // - 1 u32: handle
  // - 1 u32: vtxCount
  // - 1 u32: triCount
  // - (vtxCount * 3 * f32): vertex positions (x, y, z)
  // - (triCount * 3 * u32): triangle indices (v0, v1, v2)

  const buffer = new ArrayBuffer(
    Uint32Array.BYTES_PER_ELEMENT * 3 +
      Float32Array.BYTES_PER_ELEMENT * vtxCount * 3 +
      Uint32Array.BYTES_PER_ELEMENT * triangleCount * 3
  );
  const u32View = new Uint32Array(buffer);
  const f32View = new Float32Array(buffer);
  u32View[0] = vtxCount;
  u32View[1] = triangleCount;
  u32View[2] = handle;
  f32View.set(outVerts, 3);
  u32View.set(outIndices, 3 + vtxCount * 3);

  return new Uint8Array(buffer);
};

let lastErr = '';

export const get_last_err = (): string => lastErr;

export const create_manifold = (verts: Float32Array, indices: Uint32Array): number => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold, Mesh } = ManifoldWasm;
  try {
    const manifold = new Manifold(new Mesh({ numProp: 3, triVerts: indices, vertProperties: verts }));
    const handle = getNewHandle();
    MeshHandles.set(handle, manifold);
    return handle;
  } catch (err) {
    if (err instanceof Error) {
      if (err.name === 'ManifoldError') {
        lastErr = `The mesh passed to an operation requiring manifold input was not manifold.\n\nManifold meshes are closed/watertight, have no edges shared by more than two faces, have consistent triangle winding orders, and have no NaN/infinite vertices.\n\nDetails: ${err.message}`;
      } else {
        lastErr = `Unexpected error converting mesh to manifold representation: ${err.message}`;
      }
    } else {
      lastErr = `Unknown error: ${err}`;
    }
    return -1;
  }
};

export const apply_boolean = (
  aHandle: number,
  aTransform: Float32Array,
  bHandle: number,
  bTransform: Float32Array,
  op: BooleanOperation,
  handleOnly: boolean
): Uint8Array => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold } = ManifoldWasm;

  const a = MeshHandles.get(aHandle)?.transform([...aTransform] as Mat4);
  if (!a) {
    throw new Error(`No mesh found for handle ${aHandle}`);
  }
  const b = MeshHandles.get(bHandle)?.transform([...bTransform] as Mat4);
  if (!b) {
    throw new Error(`No mesh found for handle ${bHandle}`);
  }

  const outManifold = (() => {
    switch (op) {
      case BooleanOperation.Union:
        return Manifold.union(a, b);
      case BooleanOperation.Intersection:
        return Manifold.intersection(a, b);
      case BooleanOperation.Difference:
        return Manifold.difference(a, b);
      default:
        op satisfies never;
        throw new Error(`Unknown boolean operation: ${op}`);
    }
  })();

  // can make use of information about src mesh for different triangles within the output in the
  // future if we want to
  //
  // based on:
  // https://github.com/elalish/manifold/blob/d013ddec3284ff706772953f46354e7a1b9f2f46/bindings/wasm/examples/three.ts#L123

  const encoded = encodeManifoldMesh(outManifold, handleOnly);

  // calling `transform` creates a new manifold, so we can delete these
  a.delete();
  b.delete();

  return encoded;
};

export const simplify = (handle: number, tolerance: number) => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const mesh = MeshHandles.get(handle);
  if (!mesh) {
    throw new Error(`No mesh found for handle ${handle}`);
  }

  const simplified = mesh.simplify(tolerance);

  return encodeManifoldMesh(simplified, false);
};

export const convex_hull = (verts: Float32Array) => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold } = ManifoldWasm;

  const vec3s: Vec3[] = [];
  for (let i = 0; i < verts.length; i += 3) {
    vec3s.push([verts[i], verts[i + 1], verts[i + 2]]);
  }
  const manifold = Manifold.hull(vec3s);

  return encodeManifoldMesh(manifold, false);
};

let splitOutput: [Uint8Array, Uint8Array] = [new Uint8Array(), new Uint8Array()];

export const split_by_plane = (
  handle: number,
  transform: Float32Array,
  planeNormalX: number,
  planeNormalY: number,
  planeNormalZ: number,
  planeOffset: number
) => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const mesh = MeshHandles.get(handle)?.transform([...transform] as Mat4);
  if (!mesh) {
    throw new Error(`No mesh found for handle ${handle}`);
  }

  const [a, b] = mesh.splitByPlane([planeNormalX, planeNormalY, planeNormalZ], planeOffset);

  const aEncoded = encodeManifoldMesh(a, false);
  const bEncoded = encodeManifoldMesh(b, false);

  // calling `transform` creates a new manifold, so we can delete the original
  mesh.delete();

  splitOutput = [aEncoded, bEncoded];
};

export const get_split_output = (i: number): Uint8Array => {
  if (i === 0) {
    return splitOutput[0];
  } else if (i === 1) {
    return splitOutput[1];
  } else {
    throw new Error('split produces exactly two outputs; split ix must be 0 or 1');
  }
};
