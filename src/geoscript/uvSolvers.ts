import { AsyncOnce } from 'src/viz/util/AsyncOnce';

// The wasm URL is configured by the caller via `setUVSolversWasmURL` rather than imported with
// `?url` here, so this module can be included in a `?worker` graph without Vite emitting a
// duplicate wasm copy.
let WasmURL: string | null = null;
export const setUVSolversWasmURL = (url: string) => {
  WasmURL = url;
};

const UVSolversWasm = new AsyncOnce(async () => {
  if (!WasmURL) {
    throw new Error('uv_solvers wasm URL not configured; call setUVSolversWasmURL() first');
  }
  const mod = await import('src/viz/wasmComp/uv_solvers');
  await mod.default(fetch(WasmURL));
  return mod;
});

export const initUVSolvers = (): Promise<void> => UVSolversWasm.get().then(() => {});

export const uv_solvers_get_is_loaded = (): boolean => UVSolversWasm.isSome();

export const uv_solvers_tube = (
  verts: Float32Array,
  indices: Uint32Array,
  scale: number,
  sharpThresholdRad: number,
  caps: boolean,
  capAngleRad: number,
  capMaxSpan: number,
  capAlignment: number,
  normalizeV: boolean,
  seamStraightness: number,
  detwist: boolean
): string => {
  if (!UVSolversWasm.isSome()) {
    return 'uv_solvers module not initialized';
  }
  return UVSolversWasm.getSync().uv_solvers_tube(
    verts,
    indices,
    scale,
    sharpThresholdRad,
    caps,
    capAngleRad,
    capMaxSpan,
    capAlignment,
    normalizeV,
    seamStraightness,
    detwist
  );
};

export const uv_solvers_strip = (
  verts: Float32Array,
  indices: Uint32Array,
  scale: number,
  sharpThresholdRad: number,
  stripAngleRad: number,
  layout: number,
  uMode: number,
  planarFallback: boolean
): string => {
  if (!UVSolversWasm.isSome()) {
    return 'uv_solvers module not initialized';
  }
  return UVSolversWasm.getSync().uv_solvers_strip(
    verts,
    indices,
    scale,
    sharpThresholdRad,
    stripAngleRad,
    layout,
    uMode,
    planarFallback
  );
};

export const uv_solvers_get_verts = (): Float32Array => UVSolversWasm.getSync().uv_solvers_get_verts();
export const uv_solvers_get_indices = (): Uint32Array => UVSolversWasm.getSync().uv_solvers_get_indices();
export const uv_solvers_get_uvs = (): Float32Array => UVSolversWasm.getSync().uv_solvers_get_uvs();
export const uv_solvers_get_tangents = (): Float32Array => UVSolversWasm.getSync().uv_solvers_get_tangents();
