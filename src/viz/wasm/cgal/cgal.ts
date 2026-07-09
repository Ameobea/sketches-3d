import { AsyncOnce } from 'src/viz/util/AsyncOnce';

// The wasm URL is configured by the caller via `setCGALWasmURL` rather than
// imported with `?url` here, so this module can be safely included in a
// `?worker` graph without Vite emitting a duplicate wasm copy that would miss
// the main-thread `<link rel=preload>`.
let WasmURL: string | null = null;
export const setCGALWasmURL = (url: string) => {
  WasmURL = url;
};

const CGALWasm = new AsyncOnce(async () => {
  if (!WasmURL) {
    throw new Error('cgal wasm URL not configured; call setCGALWasmURL() first');
  }
  // Kick off wasm fetch in parallel with the JS glue fetch so the two don't
  // waterfall. The wasm URL is hashed, so any `<link rel="preload">` for it
  // makes this a cache hit.
  const wasmBinaryP = fetch(WasmURL).then(r => r.arrayBuffer());
  const mod = await import('./index.js');
  const wasmBinary = await wasmBinaryP;
  return mod.CGAL({ wasmBinary, locateFile: (_path: string) => WasmURL! } as any);
});

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

/**
 * Multi-subpath / refining CDT.  Supports holes (each subpath inserted as its own
 * closed constraint loop; nesting determines in/out) and optional size-bounded
 * Delaunay refinement.
 *
 * @param vertices flat [x0, y0, x1, y1, ...] across all subpaths
 * @param subpathLengths vertex count per subpath; their sum must equal vertices.length/2
 * @param maxEdgeLen if refine=true, upper bound on triangle edge length
 * @param minAngleBound if refine=true, shape bound passed to Delaunay_mesh_size_criteria_2 (0.125 ≈ 20.6°)
 * @param refine if true, run Delaunay_mesher_2 refinement after marking the domain.  When false, the
 *   strict input-vertex-to-output-vertex mapping is preserved; when true, Steiner points may appear
 *   and the mapping is empty.
 * @param interiorPoints flat [x0, y0, ...] of free interior points inserted (unconstrained) before
 *   domain marking — used to drive distortion-aware interior refinement.  Must lie inside the domain.
 *   Their presence empties the vertex mapping (like refinement).
 * @returns true on success; use cgal_get_cdt2d_* to retrieve the result
 */
export const cgal_triangulate_polygon_2d_with_holes = (
  vertices: Float32Array,
  subpathLengths: Uint32Array,
  maxEdgeLen: number,
  minAngleBound: number,
  refine: boolean,
  interiorPoints: Float32Array
): boolean => {
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
    HEAPF32().subarray(ptr / 4, ptr / 4 + vals.length).set(vals);
    return vec;
  };
  const vec_u32 = (vals: Uint32Array): any => {
    const vec = new CGAL.vector$uint32_t$();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    HEAPU32().subarray(ptr / 4, ptr / 4 + vals.length).set(vals);
    return vec;
  };

  const from_vec_f32 = (vec: any): Float32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPF32().subarray(ptr / 4, ptr / 4 + length).slice();
  };
  const from_vec_uint32 = (vec: any): Uint32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPU32().subarray(ptr / 4, ptr / 4 + length).slice();
  };
  const from_vec_int32 = (vec: any): Int32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAP32().subarray(ptr / 4, ptr / 4 + length).slice();
  };

  const inputVec = vec_f32(vertices);
  const lensVec = vec_u32(subpathLengths);
  const interiorVec = vec_f32(interiorPoints);
  let result: any;

  try {
    result = CGAL.triangulatePolygon2DWithHoles(
      inputVec,
      lensVec,
      maxEdgeLen,
      minAngleBound,
      refine,
      interiorVec
    );
  } catch (err) {
    inputVec.delete();
    lensVec.delete();
    interiorVec.delete();
    LastBuildPolymeshError = `CDT2DWithHoles exception: ${err}`;
    return false;
  }

  inputVec.delete();
  lensVec.delete();
  interiorVec.delete();

  if (!result.success()) {
    LastBuildPolymeshError = result.getError();
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

interface PathBoolean2DOutput {
  coords: Float32Array;
  pathLengths: Uint32Array;
}

let PathBoolean2DOutput: PathBoolean2DOutput | null = null;

/**
 * Exact-arithmetic 2D path boolean using CGAL `Polygon_set_2` over the
 * `Exact_predicates_exact_constructions_kernel` (EPECK).  Inputs and outputs
 * use the flat-coords + per-subpath-lengths format; within an input, subpaths
 * combine under even-odd / XOR semantics (matching the nesting-based fill model
 * used by `tessellate_path` CGAL backend).
 *
 * @param op 0=union, 1=intersect, 2=difference (subject minus clip), 3=xor
 * @returns true on success; use `cgal_get_path_boolean_2d_*` to retrieve output
 */
export const cgal_path_boolean_2d = (
  subjectCoords: Float32Array,
  subjectPathLengths: Uint32Array,
  clipCoords: Float32Array,
  clipPathLengths: Uint32Array,
  op: number
): boolean => {
  if (!CGALWasm.isSome()) {
    throw new Error('CGALWasm not initialized');
  }

  const CGAL = CGALWasm.getSync();

  const HEAPF32 = () => CGAL.HEAPF32 as Float32Array;
  const HEAPU32 = () => CGAL.HEAPU32 as Uint32Array;

  const vec_f32 = (vals: Float32Array): any => {
    const vec = new CGAL.vector$float$();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    HEAPF32().subarray(ptr / 4, ptr / 4 + vals.length).set(vals);
    return vec;
  };
  const vec_u32 = (vals: Uint32Array): any => {
    const vec = new CGAL.vector$uint32_t$();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    HEAPU32().subarray(ptr / 4, ptr / 4 + vals.length).set(vals);
    return vec;
  };

  const from_vec_f32 = (vec: any): Float32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPF32().subarray(ptr / 4, ptr / 4 + length).slice();
  };
  const from_vec_uint32 = (vec: any): Uint32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPU32().subarray(ptr / 4, ptr / 4 + length).slice();
  };

  const subjVec = vec_f32(subjectCoords);
  const subjLensVec = vec_u32(subjectPathLengths);
  const clipVec = vec_f32(clipCoords);
  const clipLensVec = vec_u32(clipPathLengths);
  let result: any;

  try {
    result = CGAL.pathBoolean2D(subjVec, subjLensVec, clipVec, clipLensVec, op);
  } catch (err) {
    subjVec.delete();
    subjLensVec.delete();
    clipVec.delete();
    clipLensVec.delete();
    LastBuildPolymeshError = `pathBoolean2D exception: ${err}`;
    return false;
  }

  subjVec.delete();
  subjLensVec.delete();
  clipVec.delete();
  clipLensVec.delete();

  if (!result.success()) {
    LastBuildPolymeshError = result.getError();
    result.delete();
    return false;
  }

  LastBuildPolymeshError = null;

  const outCoords = result.getCoords();
  const outLens = result.getPathLengths();

  PathBoolean2DOutput = {
    coords: from_vec_f32(outCoords),
    pathLengths: from_vec_uint32(outLens),
  };

  outCoords.delete();
  outLens.delete();
  result.delete();

  return true;
};

export const cgal_get_path_boolean_2d_coords = (): Float32Array => {
  if (!PathBoolean2DOutput) {
    throw new Error('No pathBoolean2D output set');
  }
  return PathBoolean2DOutput.coords;
};

export const cgal_get_path_boolean_2d_path_lengths = (): Uint32Array => {
  if (!PathBoolean2DOutput) {
    throw new Error('No pathBoolean2D output set');
  }
  return PathBoolean2DOutput.pathLengths;
};

export const cgal_clear_path_boolean_2d_output = (): void => {
  PathBoolean2DOutput = null;
};
