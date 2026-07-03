import { AsyncOnce } from 'src/viz/util/AsyncOnce';

// The wasm URL is configured by the caller via `setUVUnwrapWasmURL` rather than imported with
// `?url` here, so this module can be included in a `?worker` graph without Vite emitting a
// duplicate wasm copy.
let WasmURL: string | null = null;
export const setUVUnwrapWasmURL = (url: string) => {
  WasmURL = url;
};

const UVUnwrapWasm = new AsyncOnce(async () => {
  if (!WasmURL) {
    throw new Error('uv_unwrap wasm URL not configured; call setUVUnwrapWasmURL() first');
  }
  const wasmBinaryP = fetch(WasmURL).then(r => r.arrayBuffer());
  const mod = await import('src/viz/wasm/uv_unwrap/uv-unwrap.js');
  const wasmBinary = await wasmBinaryP;
  return (mod.UVUnwrap as any)({ wasmBinary, locateFile: (_path: string) => WasmURL! });
});

export const initUVUnwrap = (): Promise<void> => UVUnwrapWasm.get().then(() => {});

export const get_uv_unwrap_loaded = (): boolean => UVUnwrapWasm.isSome();

let lastVerts: Float32Array = new Float32Array(0);
let lastIndices: Uint32Array = new Uint32Array(0);
let lastUvs: Float32Array = new Float32Array(0);
let lastTangents: Float32Array = new Float32Array(0);

/**
 * Runs a BFF UV unwrap on the given indexed mesh, stashing the (re-indexed) output for retrieval via
 * the `uv_unwrap_get_*` getters.  Returns an error string, or `''` on success.  Called synchronously
 * from the geoscript wasm; the module must already be initialized.
 */
export const unwrap_uvs = (
  verts: Float32Array,
  indices: Uint32Array,
  nCones: number,
  flattenToDisk: boolean,
  mapToSphere: boolean,
  islandRotation: boolean
): string => {
  if (!UVUnwrapWasm.isSome()) {
    return 'uv_unwrap module not initialized';
  }
  const UVUnwrap = UVUnwrapWasm.getSync();

  const HEAPF32 = () => UVUnwrap.HEAPF32 as Float32Array;
  const HEAPU32 = () => UVUnwrap.HEAPU32 as Uint32Array;

  const vec_generic = (
    vecCtor: new () => any,
    mem: () => Float32Array | Uint32Array,
    vals: Float32Array | Uint32Array
  ) => {
    const vec = new vecCtor();
    vec.resize(vals.length, 0);
    const ptr = vec.data();
    const buf = mem().subarray(ptr / 4, ptr / 4 + vals.length);
    buf.set(vals);
    return vec;
  };
  const vec_f32 = (vals: Float32Array) => vec_generic(UVUnwrap.vector$float$, HEAPF32, vals);
  const vec_uint32 = (vals: Uint32Array) => vec_generic(UVUnwrap.vector$uint32_t$, HEAPU32, vals);

  // copy out of the wasm heap (a later call could grow/realloc it)
  const from_vec_f32 = (vec: any): Float32Array => {
    const len = vec.size();
    const ptr = vec.data();
    const out = new Float32Array(len);
    out.set(HEAPF32().subarray(ptr / 4, ptr / 4 + len));
    return out;
  };
  const from_vec_u32 = (vec: any): Uint32Array => {
    const len = vec.size();
    const ptr = vec.data();
    const out = new Uint32Array(len);
    out.set(HEAPU32().subarray(ptr / 4, ptr / 4 + len));
    return out;
  };

  const vec_verts = vec_f32(verts);
  const vec_indices = vec_uint32(indices);
  let output: any = null;
  try {
    output = UVUnwrap.unwrapUVs(vec_indices, vec_verts, nCones, flattenToDisk, mapToSphere, islandRotation);
    const error: string = output.error;
    if (error) {
      return error;
    }
    lastUvs = from_vec_f32(output.uvs);
    lastVerts = from_vec_f32(output.verts);
    lastIndices = from_vec_u32(output.indices);
    lastTangents = from_vec_f32(output.tangents);
    return '';
  } catch (err) {
    return err instanceof Error ? err.message : String(err);
  } finally {
    vec_verts.delete();
    vec_indices.delete();
    output?.delete();
  }
};

export const uv_unwrap_get_verts = (): Float32Array => lastVerts;
export const uv_unwrap_get_indices = (): Uint32Array => lastIndices;
export const uv_unwrap_get_uvs = (): Float32Array => lastUvs;
export const uv_unwrap_get_tangents = (): Float32Array => lastTangents;
