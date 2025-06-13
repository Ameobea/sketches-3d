import ManifoldModule, { type Manifold, type ManifoldToplevel, type Vec3 } from 'manifold-3d';
import manifoldWasURL from 'manifold-3d/manifold.wasm?url';

// import ManifoldModule from './manifoldComp/manifold';
// import type { Manifold, ManifoldToplevel, Vec3 } from 'manifold-3d';
// import manifoldWasURL from './manifoldComp/manifold.wasm?url';

let ManifoldWasm: ManifoldToplevel | null = null;

export const initManifoldWasm = async () => {
  if (ManifoldWasm) {
    return ManifoldWasm;
  }

  ManifoldWasm = await ManifoldModule({ locateFile: () => manifoldWasURL });
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
  const outVerts = outMesh.vertProperties;
  const outIndices = outMesh.triVerts;

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

export const create_manifold = (verts: Float32Array, indices: Uint32Array): number => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold, Mesh } = ManifoldWasm;
  const manifold = new Manifold(new Mesh({ numProp: 3, triVerts: indices, vertProperties: verts }));
  const handle = getNewHandle();
  MeshHandles.set(handle, manifold);
  return handle;
};

export const apply_boolean = (
  aHandle: number,
  bHandle: number,
  op: BooleanOperation,
  handleOnly: boolean
): Uint8Array => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold } = ManifoldWasm;

  const a = MeshHandles.get(aHandle);
  if (!a) {
    throw new Error(`No mesh found for handle ${aHandle}`);
  }
  const b = MeshHandles.get(bHandle);
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

  return encodeManifoldMesh(outManifold, handleOnly);
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
