import { AsyncOnce } from 'src/viz/util/AsyncOnce';

// The wasm URL is configured by the caller via `setClipper2WasmURL` rather
// than imported with `?url` here, so this module can be safely included in a
// `?worker` graph without Vite emitting a duplicate wasm copy that would miss
// the main-thread `<link rel=preload>`.
let WasmURL: string | null = null;
export const setClipper2WasmURL = (url: string) => {
  WasmURL = url;
};

const Clipper2Wasm = new AsyncOnce(async () => {
  if (!WasmURL) {
    throw new Error('clipper2 wasm URL not configured; call setClipper2WasmURL() first');
  }
  const wasmBinaryP = fetch(WasmURL).then(r => r.arrayBuffer());
  const mod = await import('./clipper2z.js');
  const wasmBinary = await wasmBinaryP;
  return mod.default({ wasmBinary, locateFile: (_path: string) => WasmURL! } as any);
});

export const initClipper2 = (): Promise<void> | true => {
  if (Clipper2Wasm.isSome()) {
    return true;
  }
  return Clipper2Wasm.get().then(() => {});
};

export const clipper2_get_is_loaded = (): boolean => Clipper2Wasm.isSome();

/**
 * Flat raw-pointer path boolean FFI.  Stages subject+clip as f32 coords / u32 subpath
 * lengths directly in the Clipper2 wasm heap (single arena, two memcpys), runs the op fully
 * in C++ (f32 -> scaled Path64 -> boolean -> simplify -> flat f32 out), then exposes the
 * output buffers as views for the caller to copy out.  Replaces the embind PathsD route,
 * which spent ~10x the actual clip time constructing/destructing per-point JS handles.
 *
 * op: 0=union 1=intersect 2=difference 3=xor 4=self-union (clip ignored)
 */
export const clipper2_boolean_flat = (
  op: number,
  fillRule: number,
  subjectCoords: Float32Array,
  subjectPathLengths: Uint32Array,
  clipCoords: Float32Array,
  clipPathLengths: Uint32Array
): void => {
  const C = Clipper2Wasm.getSync();
  const arenaPtr =
    C._c2_stage_input(
      subjectCoords.length,
      subjectPathLengths.length,
      clipCoords.length,
      clipPathLengths.length
    ) >>> 0;
  // fetch heap views only after the (possibly memory-growing) alloc call
  const base = arenaPtr >> 2;
  C.HEAPF32.set(subjectCoords, base);
  C.HEAPF32.set(clipCoords, base + subjectCoords.length);
  const lensBase = base + subjectCoords.length + clipCoords.length;
  C.HEAPU32.set(subjectPathLengths, lensBase);
  C.HEAPU32.set(clipPathLengths, lensBase + subjectPathLengths.length);
  C._c2_boolean_flat(op, fillRule);
};

// Views into the clipper2 heap; only valid until the next clipper2 call.  wasm-bindgen
// copies them into the geoscript heap immediately on return, which is the only consumer.
export const clipper2_get_output_coords_f32 = (): Float32Array => {
  const C = Clipper2Wasm.getSync();
  const count = C._c2_output_coord_count() >>> 0;
  const ptr = C._c2_output_coords_ptr() >>> 0;
  return C.HEAPF32.subarray(ptr >> 2, (ptr >> 2) + count);
};

export const clipper2_get_output_path_lengths_flat = (): Uint32Array => {
  const C = Clipper2Wasm.getSync();
  const count = C._c2_output_path_count() >>> 0;
  const ptr = C._c2_output_path_lens_ptr() >>> 0;
  return C.HEAPU32.subarray(ptr >> 2, (ptr >> 2) + count);
};

export const clipper2_clear_output_flat = (): void => {
  Clipper2Wasm.getSync()._c2_clear_output();
};

/**
 * Flat offset FFI (same scheme as the boolean API). `pathIsClosed` is per-subpath; closed
 * paths get Polygon end caps and open paths use `endType`. All the join/end shaping params
 * pass straight through to the forked ClipperOffset.
 */
export const clipper2_offset_flat = (
  coords: Float32Array,
  pathLengths: Uint32Array,
  pathIsClosed: Uint32Array,
  delta: number,
  joinType: number,
  endType: number,
  miterLimit: number,
  arcTolerance: number,
  preserveCollinear: boolean,
  reverseSolution: boolean,
  stepCount: number,
  superellipseExponent: number,
  endExtensionScale: number,
  arrowBackSweep: number,
  teardropPinch: number,
  joinAngleThreshold: number,
  chebyshevSpacing: boolean,
  simplifyEpsilon: number
): void => {
  const C = Clipper2Wasm.getSync();
  const arenaPtr = (C._c2_stage_offset(coords.length, pathLengths.length) >>> 0) >> 2;
  C.HEAPF32.set(coords, arenaPtr);
  C.HEAPU32.set(pathLengths, arenaPtr + coords.length);
  C.HEAPU32.set(pathIsClosed, arenaPtr + coords.length + pathLengths.length);
  C._c2_offset_flat(
    delta,
    joinType,
    endType,
    miterLimit,
    arcTolerance,
    preserveCollinear ? 1 : 0,
    reverseSolution ? 1 : 0,
    stepCount,
    superellipseExponent,
    endExtensionScale,
    arrowBackSweep,
    teardropPinch,
    joinAngleThreshold,
    chebyshevSpacing ? 1 : 0,
    simplifyEpsilon
  );
};
