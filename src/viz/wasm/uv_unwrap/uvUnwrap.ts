import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import WasmURL from './uv-unwrap.wasm?url';

const UVUnwrapWasm = new AsyncOnce(() =>
  import('./uv-unwrap.js')
    .then(mod => {
      (mod.UVUnwrap as any).locateFile = (_path: string) => WasmURL;
      return mod.UVUnwrap;
    })
    .then(mod => mod({ locateFile: (_path: string) => WasmURL }))
);

export const initUVUnwrap = (): Promise<void> | true => {
  if (UVUnwrapWasm.isSome()) {
    return true;
  }
  return UVUnwrapWasm.get().then(() => {});
};

export const getIsUVUnwrapLoaded = (): boolean => UVUnwrapWasm.isSome();

const unwrapInner = (
  verts: Float32Array,
  indices: Uint32Array,
  nCones: number,
  flattenToDisk: boolean,
  mapToSphere: boolean,
  enableUVIslandRotation: boolean
) => {
  if (!UVUnwrapWasm.isSome()) {
    throw new Error('UVUnwrapWasm not initialized');
  }

  const UVUnwrap = UVUnwrapWasm.getSync();

  const HEAPF32 = () => UVUnwrap.HEAPF32 as Float32Array;
  const HEAPU32 = () => UVUnwrap.HEAPU32 as Uint32Array;

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

  const vec_f32 = (vals: number[] | Float32Array) => vec_generic(UVUnwrap.vector$float$, HEAPF32, vals);

  const vec_uint32 = (vals: number[] | Uint32Array | Uint16Array) =>
    vec_generic(UVUnwrap.vector$uint32_t$, HEAPU32, vals);

  const vec_verts = vec_f32(verts);
  const vec_indices = vec_uint32(indices);

  const output = UVUnwrap.unwrapUVs(
    vec_indices,
    vec_verts,
    nCones,
    flattenToDisk,
    mapToSphere,
    enableUVIslandRotation
  );

  return {
    output,
    vec_verts,
    vec_indices,
    HEAPF32,
    HEAPU32,
  };
};

export const unwrapUVs = (
  verts: Float32Array,
  indices: Uint32Array,
  nCones: number,
  flattenToDisk: boolean,
  mapToSphere: boolean,
  enableUVIslandRotation: boolean
):
  | {
      type: 'ok';
      out: {
        uvs: Float32Array;
        verts: Float32Array;
        indices: Uint32Array;
      };
    }
  | { type: 'error'; message: string } => {
  const { output, vec_verts, vec_indices, HEAPF32, HEAPU32 } = unwrapInner(
    verts,
    indices,
    nCones,
    flattenToDisk,
    mapToSphere,
    enableUVIslandRotation
  );

  const from_vec_f32 = (vec: any): Float32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPF32().subarray(ptr / 4, ptr / 4 + length);
  };

  const from_vec_u32 = (vec: any): Uint32Array => {
    const length = vec.size();
    const ptr = vec.data();
    return HEAPU32().subarray(ptr / 4, ptr / 4 + length);
  };

  const error = output.error;
  if (error) {
    console.error('UV Unwrap error:', error);
    vec_verts.delete();
    vec_indices.delete();
    output.delete();
    return { type: 'error', message: error };
  }
  const uvs = from_vec_f32(output.uvs).slice();
  const unwrappedVerts = from_vec_f32(output.verts).slice();
  const unwrappedIndices = from_vec_u32(output.indices).slice();

  vec_verts.delete();
  vec_indices.delete();
  output.delete();

  return {
    type: 'ok',
    out: {
      uvs,
      verts: unwrappedVerts,
      indices: unwrappedIndices,
    },
  };
};

export const buildUVUnwrapDistortionSVG = (
  verts: Float32Array,
  indices: Uint32Array,
  nCones: number,
  flattenToDisk: boolean,
  mapToSphere: boolean,
  enableUVIslandRotation: boolean
): { type: 'ok'; out: string } | { type: 'error'; message: string } => {
  const { output, vec_verts, vec_indices } = unwrapInner(
    verts,
    indices,
    nCones,
    flattenToDisk,
    mapToSphere,
    enableUVIslandRotation
  );

  const error = output.error;
  if (error) {
    console.error('UV Unwrap error:', error);
    vec_verts.delete();
    vec_indices.delete();
    output.delete();
    return { type: 'error', message: error };
  }

  const svg = output.getDistortionSvg();
  vec_verts.delete();
  vec_indices.delete();
  output.delete();

  return {
    type: 'ok',
    out: svg,
  };
};
