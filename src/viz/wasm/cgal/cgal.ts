import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import WasmURL from './index.wasm?url';

const CGALWasm = new AsyncOnce(() =>
  import('./index.js')
    .then(mod => {
      (mod.CGAL as any).locateFile = (_path: string) => WasmURL;
      return mod.CGAL;
    })
    .then(mod => mod({ locateFile: (_path: string) => WasmURL }))
);

export const initCGAL = (): Promise<void> | true => {
  if (CGALWasm.isSome()) {
    return true;
  }
  return CGALWasm.get().then(() => {});
};

export const getIsCGALLoaded = (): boolean => CGALWasm.isSome();

const buildCGALPolymesh = (verts: Float32Array, indices: Uint32Array) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const CGAL = CGALWasm.getSync();

  const HEAPF32 = () => CGAL.HEAPF32 as Float32Array;
  const HEAPU32 = () => CGAL.HEAPU32 as Uint32Array;

  const vec_generic = (
    vecCtor: new () => any,
    mem: () => Float32Array | Uint32Array,
    vals: number[] | Float32Array | Uint32Array | Uint16Array
  ) => {
    const vec = new vecCtor();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    const buf = mem().subarray(ptr / 4, ptr / 4 + vals.length);
    buf.set(vals);
    return vec;
  };

  const vec_f32 = (vals: number[] | Float32Array) => vec_generic(CGAL.vector$float$, HEAPF32, vals);

  const vec_uint32 = (vals: number[] | Uint32Array | Uint16Array) =>
    vec_generic(CGAL.vector$uint32_t$, HEAPU32, vals);

  const vec_verts = vec_f32(verts);
  const vec_indices = vec_uint32(indices);

  const mesh = new CGAL.PolyMesh();
  mesh.buildFromBuffers(vec_verts, vec_indices);

  vec_verts.delete();
  vec_indices.delete();

  return mesh;
};

let OutputMesh: { verts: Float32Array; indices: Uint32Array } | null = null;

const setOutputMeshFromCGALMesh = (mesh: any): void => {
  const CGAL = CGALWasm.getSync();

  const HEAPF32 = () => CGAL.HEAPF32 as Float32Array;
  const HEAPU32 = () => CGAL.HEAPU32 as Uint32Array;

  const from_vec_f32 = (vec: any): Float32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPF32().subarray(ptr / 4, ptr / 4 + length);
  };

  const from_vec_uint32 = (vec: any): Uint32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPU32().subarray(ptr / 4, ptr / 4 + length);
  };

  const indices = mesh.getIndices();
  const verts = mesh.getVertices();

  const out = { indices: from_vec_uint32(indices).slice(), verts: from_vec_f32(verts).slice() };

  indices.delete();
  verts.delete();

  OutputMesh = out;
};

export const cgal_alpha_wrap_mesh = (
  verts: Float32Array,
  indices: Uint32Array,
  relativeAlpha: number,
  relativeOffset: number
) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);

  let wrapped;
  try {
    wrapped = mesh.alphaWrap(relativeAlpha, relativeOffset);
  } catch (err) {
    console.error('Error during CGAL alpha wrap:', err);
    throw err;
  }

  mesh.delete();

  setOutputMeshFromCGALMesh(wrapped);
  wrapped.delete();
};

export const cgal_alpha_wrap_points = (
  points: Float32Array,
  relativeAlpha: number,
  relativeOffset: number
) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const CGAL = CGALWasm.getSync();

  // TODO: de-dupe all these helpers
  const HEAPF32 = () => CGAL.HEAPF32 as Float32Array;

  const vec_generic = (vecCtor: new () => any, mem: () => Float32Array, vals: number[] | Float32Array) => {
    const vec = new vecCtor();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    const buf = mem().subarray(ptr / 4, ptr / 4 + vals.length);
    buf.set(vals);
    return vec;
  };

  const vec_f32 = (vals: number[] | Float32Array) => vec_generic(CGAL.vector$float$, HEAPF32, vals);

  const vec_points = vec_f32(points);

  const wrapped = CGAL.alphaWrapPointCloud(vec_points, relativeAlpha, relativeOffset);

  vec_points.delete();

  setOutputMeshFromCGALMesh(wrapped);
  wrapped.delete();
};

export const cgal_get_output_mesh_verts = (): Float32Array => {
  if (!OutputMesh) {
    throw new Error('No CGAL output mesh set');
  }
  return OutputMesh.verts;
};

export const cgal_get_output_mesh_indices = (): Uint32Array => {
  if (!OutputMesh) {
    throw new Error('No CGAL output mesh set');
  }
  return OutputMesh.indices;
};

export const cgal_clear_output_mesh = (): void => {
  OutputMesh = null;
};

export const cgal_catmull_smooth_mesh = (verts: Float32Array, indices: Uint32Array, iterations: number) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  mesh.catmull_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_loop_smooth_mesh = (verts: Float32Array, indices: Uint32Array, iterations: number) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  mesh.loop_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_doosabin_smooth_mesh = (verts: Float32Array, indices: Uint32Array, iterations: number) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  mesh.dooSabin_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_sqrt_smooth_mesh = (verts: Float32Array, indices: Uint32Array, iterations: number) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  mesh.sqrt_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};
