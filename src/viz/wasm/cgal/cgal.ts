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

export const cgal_get_is_loaded = (): boolean => getIsCGALLoaded();

let LastBuildPolymeshError: string | null = null;

export const cgal_get_last_error = (): string | null => LastBuildPolymeshError;

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
  try {
    const ok = mesh.buildFromBuffers(vec_verts, vec_indices);
    if (!ok) {
      const err = mesh.getLastError()
      LastBuildPolymeshError = err;
      throw new Error(err);
    } else {
      LastBuildPolymeshError = null;
    }
    return mesh;
  } catch(err) {
    console.error('Error building CGAL PolyMesh:', err);
    mesh.delete();
    return null
  } finally {
    vec_verts.delete();
    vec_indices.delete();
  }
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

  mesh.maybe_triangulate();

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
  if (!mesh) {
    return;
  }

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
  if (!mesh) {
    return;
  }
  mesh.catmull_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_loop_smooth_mesh = (verts: Float32Array, indices: Uint32Array, iterations: number) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  if (!mesh) {
    return;
  }
  mesh.loop_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_doosabin_smooth_mesh = (verts: Float32Array, indices: Uint32Array, iterations: number) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  if (!mesh) {
    return;
  }
  mesh.dooSabin_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_sqrt_smooth_mesh = (verts: Float32Array, indices: Uint32Array, iterations: number) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  if (!mesh) {
    return;
  }
  mesh.sqrt_smooth(iterations);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_remesh_planar_patches = (
  verts: Float32Array,
  indices: Uint32Array,
  maxAngleDegrees: number,
  maxOffset: number
) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  if (!mesh) {
    return;
  }
  mesh.remesh_planar_patches(maxAngleDegrees, maxOffset);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_remesh_isotropic = (
  verts: Float32Array,
  indices: Uint32Array,
  targetEdgeLength: number,
  iterations: number,
  protectBorders: boolean,
  autoSharpEdges: boolean,
  sharpAngleThresholdDegrees: number
) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  if (!mesh) {
    return;
  }
  mesh.isotropic_remesh(
    targetEdgeLength,
    iterations,
    protectBorders,
    autoSharpEdges,
    sharpAngleThresholdDegrees
  );

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export const cgal_remesh_delaunay = (
  verts: Float32Array,
  indices: Uint32Array,
  targetEdgeLength: number,
  facetDistance: number,
  autoSharpEdges: boolean,
  sharpAngleThresholdDegrees: number
) => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const mesh = buildCGALPolymesh(verts, indices);
  if (!mesh) {
    return;
  }
  mesh.delaunay_remesh(targetEdgeLength, facetDistance, autoSharpEdges, sharpAngleThresholdDegrees);

  setOutputMeshFromCGALMesh(mesh);
  mesh.delete();
};

export interface CDT2DResult {
  vertices: Float32Array;
  indices: Uint32Array;
  /**
   * in vtx ix -> out vtx ix mapping
   */
  vertexMapping: Int32Array;
}

let CDT2DOutput: CDT2DResult | null = null;

/**
 * Triangulates a 2D polygon using Constrained Delaunay Triangulation.  Guarantees that input vertices
 * are preserved in the output so that this result can be used as part of the a process to build 2-manifold
 * 3D meshes.
 *
 * @param vertices 2D points [x0, y0, x1, y1, ...] defining the path in CCW winding order
 * @returns true if triangulation succeeded, false if it failed (errors retrievable via `cgal_get_last_error`)
 *
 * Use cgal_get_cdt2d_* functions to retrieve the result after a successful call.
 */
export const cgal_triangulate_polygon_2d = (vertices: Float32Array): boolean => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const CGAL = CGALWasm.getSync();

  const HEAPF32 = () => CGAL.HEAPF32 as Float32Array;
  const HEAPU32 = () => CGAL.HEAPU32 as Uint32Array;
  const HEAP32 = () => CGAL.HEAP32 as Int32Array;

  const vec_f32 = (vals: Float32Array): any => {
    const vec = new CGAL.vector$float$();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    const buf = HEAPF32().subarray(ptr / 4, ptr / 4 + vals.length);
    buf.set(vals);
    return vec;
  };

  const from_vec_f32 = (vec: any): Float32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPF32()
      .subarray(ptr / 4, ptr / 4 + length)
      .slice();
  };

  const from_vec_uint32 = (vec: any): Uint32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPU32()
      .subarray(ptr / 4, ptr / 4 + length)
      .slice();
  };

  const from_vec_int32 = (vec: any): Int32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAP32()
      .subarray(ptr / 4, ptr / 4 + length)
      .slice();
  };

  const inputVec = vec_f32(vertices);
  let result: any;

  try {
    result = CGAL.triangulatePolygon2D(inputVec);
  } catch (err) {
    inputVec.delete();
    LastBuildPolymeshError = `CDT2D exception: ${err}`;
    return false;
  }

  inputVec.delete();

  if (!result.success()) {
    const error = result.getError();
    LastBuildPolymeshError = error;
    result.delete();
    return false;
  }

  LastBuildPolymeshError = null;

  const outVerts = result.getVertices();
  const outIndices = result.getIndices();
  const outMapping = result.getVertexMapping();

  CDT2DOutput = {
    vertices: from_vec_f32(outVerts),
    indices: from_vec_uint32(outIndices),
    vertexMapping: from_vec_int32(outMapping),
  };

  outVerts.delete();
  outIndices.delete();
  outMapping.delete();
  result.delete();

  return true;
};

export const cgal_get_cdt2d_vertices = (): Float32Array => {
  if (!CDT2DOutput) {
    throw new Error('No CDT2D output set');
  }
  return CDT2DOutput.vertices;
};

export const cgal_get_cdt2d_indices = (): Uint32Array => {
  if (!CDT2DOutput) {
    throw new Error('No CDT2D output set');
  }
  return CDT2DOutput.indices;
};

export const cgal_get_cdt2d_vertex_mapping = (): Int32Array => {
  if (!CDT2DOutput) {
    throw new Error('No CDT2D output set');
  }
  return CDT2DOutput.vertexMapping;
};

export const cgal_clear_cdt2d_output = (): void => {
  CDT2DOutput = null;
};
