import ManifoldModule, { type ManifoldToplevel } from 'manifold-3d';
import manifoldWasURL from 'manifold-3d/manifold.wasm?url';
import * as THREE from 'three';

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

export const apply_boolean = (
  aVerts: Float32Array,
  aIndices: Uint32Array,
  bVerts: Float32Array,
  bIndices: Uint32Array,
  op: BooleanOperation
): Uint8Array => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold, Mesh } = ManifoldWasm;

  const a = new Manifold(new Mesh({ numProp: 3, triVerts: aIndices, vertProperties: aVerts }));
  const b = new Manifold(new Mesh({ numProp: 3, triVerts: bIndices, vertProperties: bVerts }));

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

  const outMesh = outManifold.getMesh();
  const outVerts = outMesh.vertProperties;
  const outIndices = outMesh.triVerts;

  const vtxCount = outVerts.length / 3;
  const triangleCount = outIndices.length / 3;

  // encode the output mesh into binary with the following structure:
  // - 1 u32: vtxCount
  // - 1 u32: triCount
  // - (vtxCount * 3 * f32): vertex positions (x, y, z)
  // - (triCount * 3 * u32): triangle indices (v0, v1, v2)

  const buffer = new ArrayBuffer(
    Uint32Array.BYTES_PER_ELEMENT +
      Uint32Array.BYTES_PER_ELEMENT +
      Float32Array.BYTES_PER_ELEMENT * vtxCount * 3 +
      Uint32Array.BYTES_PER_ELEMENT * triangleCount * 3
  );
  const u32View = new Uint32Array(buffer);
  const f32View = new Float32Array(buffer);
  u32View[0] = vtxCount;
  u32View[1] = triangleCount;
  f32View.set(outVerts, 2);
  u32View.set(outIndices, 2 + vtxCount * 3);

  a.delete();
  b.delete();
  outManifold.delete();

  return new Uint8Array(buffer);
};
