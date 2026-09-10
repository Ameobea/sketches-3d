import ManifoldModule, { type Manifold, type ManifoldToplevel, type Mat4, type Vec3 } from 'manifold-3d';

// The wasm URL is configured by the caller via `setManifoldWasmURL` rather than
// imported with `?url` here.  Keeping the import out of this module's graph
// prevents Vite from emitting a duplicate copy of the wasm under
// `workers/assets/` when this module is pulled in from a `?worker` entry point,
// which would defeat the main-thread `<link rel=preload>` for it.
let manifoldWasmURL: string | null = null;
export const setManifoldWasmURL = (url: string) => {
  manifoldWasmURL = url;
};

let ManifoldWasm: ManifoldToplevel | null = null;

export const initManifoldWasm = async () => {
  if (ManifoldWasm) {
    return ManifoldWasm;
  }
  if (!manifoldWasmURL) {
    throw new Error('manifold wasm URL not configured; call setManifoldWasmURL() first');
  }

  ManifoldWasm = await ManifoldModule({ locateFile: () => manifoldWasmURL! });
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

const HEADER_WORDS = 6;

// Layout is documented on the Rust side: `mesh_boolean.rs::decode_manifold_output`.
const encodeManifoldMesh = (manifold: Manifold, handleOnly: boolean) => {
  const handle = getNewHandle();
  MeshHandles.set(handle, manifold);

  if (handleOnly) {
    const u32View = new Uint32Array(HEADER_WORDS);
    u32View[2] = handle;
    return new Uint8Array(u32View.buffer);
  }

  const outMesh = manifold.getMesh();
  const { numProp, vertProperties, triVerts, runIndex, runTransform, runOriginalID } = outMesh;
  const { mergeFromVert, mergeToVert } = outMesh;
  const vtxCount = vertProperties.length / numProp;
  const triangleCount = triVerts.length / 3;
  const numRun = outMesh.numRun;
  const runIndexLen = numRun > 0 ? numRun + 1 : 0;

  const words =
    HEADER_WORDS +
    vertProperties.length +
    triVerts.length +
    runIndexLen +
    numRun * 12 +
    numRun * 2 +
    mergeFromVert.length * 2;
  const buffer = new ArrayBuffer(words * 4);
  const u32View = new Uint32Array(buffer);
  const f32View = new Float32Array(buffer);
  u32View.set([vtxCount, triangleCount, handle, numProp, numRun, mergeFromVert.length]);
  let at = HEADER_WORDS;
  f32View.set(vertProperties, at);
  at += vertProperties.length;
  u32View.set(triVerts, at);
  at += triVerts.length;
  u32View.set(runIndex.subarray(0, runIndexLen), at);
  at += runIndexLen;
  for (let run = 0; run < numRun; run += 1) {
    if (runTransform.length >= (run + 1) * 12) {
      f32View.set(runTransform.subarray(run * 12, run * 12 + 12), at);
    } else {
      f32View.set([1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0], at);
    }
    at += 12;
  }
  for (let run = 0; run < numRun; run += 1) {
    u32View[at + run] = outMesh.backside(run) ? 1 : 0;
  }
  at += numRun;
  u32View.set(runOriginalID.subarray(0, numRun), at);
  at += numRun;
  u32View.set(mergeFromVert, at);
  at += mergeFromVert.length;
  u32View.set(mergeToVert, at);

  return new Uint8Array(buffer);
};

let lastErr = '';

export const get_last_err = (): string => lastErr;

export const create_manifold = (
  vertProperties: Float32Array,
  numProp: number,
  triVerts: Uint32Array,
  mergeFromVert: Uint32Array,
  mergeToVert: Uint32Array
): number => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold, Mesh } = ManifoldWasm;
  try {
    const mesh = new Mesh({
      numProp,
      vertProperties,
      triVerts,
      ...(mergeFromVert.length > 0 ? { mergeFromVert, mergeToVert } : {}),
    });
    const manifold = new Manifold(mesh);
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

// Re-lays an operand's property lanes onto the boolean's union layout (see `lane_map` in Rust).
const widenProps = (m: Manifold, laneMap: Int32Array, numProp: number): Manifold => {
  const lanes = numProp - 3;
  const identity = m.numProp() === lanes && laneMap.every((src, k) => src === k);
  if (identity) return m;
  const widened = m.setProperties(lanes, (newProp, _pos, oldProp) => {
    for (let k = 0; k < lanes; k += 1) {
      newProp[k] = laneMap[k] < 0 ? 0 : oldProp[laneMap[k]];
    }
  });
  m.delete();
  return widened;
};

export const apply_boolean = (
  aHandle: number,
  aTransform: Float32Array,
  aLaneMap: Int32Array,
  bHandle: number,
  bTransform: Float32Array,
  bLaneMap: Int32Array,
  numProp: number,
  op: BooleanOperation,
  handleOnly: boolean
): Uint8Array => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold } = ManifoldWasm;

  const a0 = MeshHandles.get(aHandle)?.transform([...aTransform] as Mat4);
  if (!a0) {
    throw new Error(`No mesh found for handle ${aHandle}`);
  }
  const b0 = MeshHandles.get(bHandle)?.transform([...bTransform] as Mat4);
  if (!b0) {
    throw new Error(`No mesh found for handle ${bHandle}`);
  }
  const a = widenProps(a0, aLaneMap, numProp);
  const b = widenProps(b0, bLaneMap, numProp);

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

/**
 * Compute the convex hull of a set of input vertices and return the resulting manifold mesh
 * directly as `{ verts, indices }`, without registering it in `MeshHandles` (the Manifold
 * is freed before returning).  Used for one-shot hull computations driven from JS rather
 * than from a chained geoscript operation.
 */
export const compute_convex_hull_mesh = (
  verts: Float32Array
): { verts: Float32Array; indices: Uint32Array } => {
  if (!ManifoldWasm) {
    throw new Error('Manifold Wasm not initialized');
  }

  const { Manifold } = ManifoldWasm;

  const vec3s: Vec3[] = [];
  for (let i = 0; i < verts.length; i += 3) {
    vec3s.push([verts[i], verts[i + 1], verts[i + 2]]);
  }
  const manifold = Manifold.hull(vec3s);
  try {
    const mesh = manifold.getMesh();
    // `vertProperties` and `triVerts` are views into Wasm memory — copy out of WASM-owned
    // buffers before deleting the Manifold so the caller can keep the result.
    return {
      verts: new Float32Array(mesh.vertProperties),
      indices: new Uint32Array(mesh.triVerts),
    };
  } finally {
    manifold.delete();
  }
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
