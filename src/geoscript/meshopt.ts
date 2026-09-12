import type { MeshoptSimplifier as SimplifierType } from 'meshoptimizer/simplifier';

let simplifier: typeof SimplifierType | null = null;
let loading: Promise<void> | null = null;

export const initMeshopt = (): Promise<void> => {
  loading ??= import('meshoptimizer/simplifier').then(async ({ MeshoptSimplifier }) => {
    await MeshoptSimplifier.ready;
    if (!MeshoptSimplifier.supported) throw new Error('Mesh simplification requires WebAssembly');
    simplifier = MeshoptSimplifier;
  });
  return loading;
};

export const meshopt_is_loaded = (): boolean => simplifier !== null;

/** Returns indices into the original vertex arrays; positions and attributes never move. */
export const meshopt_simplify = (
  indices: Uint32Array,
  positions: Float32Array,
  attributes: Float32Array,
  weights: Float32Array,
  vertexLocks: Uint8Array,
  tolerance: number
): Uint32Array => {
  try {
    if (!simplifier) throw new Error('meshopt is not initialized');
    if (!(tolerance > 0) || !Number.isFinite(tolerance)) throw new Error('Invalid tolerance');
    if (positions.length % 3 || indices.length % 3) throw new Error('Invalid triangle buffers');
    if (!positions.every(Number.isFinite) || !attributes.every(Number.isFinite)) {
      throw new Error('Mesh positions and attributes must be finite');
    }
    if (weights.length > 32 || attributes.length !== (positions.length / 3) * weights.length) {
      throw new Error('Invalid attribute buffers');
    }
    if (vertexLocks.length !== positions.length / 3) throw new Error('Invalid vertex locks');
    const [result] = simplifier.simplifyWithAttributes(
      indices,
      positions,
      3,
      attributes,
      weights.length,
      Array.from(weights),
      vertexLocks,
      0,
      tolerance,
      ['ErrorAbsolute', 'RegularizeLight', 'LockBorder']
    );
    return result;
  } catch (err) {
    // wasm-bindgen's catch boundary can carry this message without losing Error.message.
    throw err instanceof Error ? err.message : String(err);
  }
};
