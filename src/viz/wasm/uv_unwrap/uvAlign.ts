import type { MeshUvAlign, UvAlignAxis } from 'src/viz/materials/schema';

const AXIS_VECS: Record<UvAlignAxis, [number, number, number]> = {
  '+x': [1, 0, 0],
  '-x': [-1, 0, 0],
  '+y': [0, 1, 0],
  '-y': [0, -1, 0],
  '+z': [0, 0, 1],
  '-z': [0, 0, -1],
};
// Extra +90° UV turns so the chosen texture axis, rather than +v, lands on the target direction.
const QUARTER_TURNS: Record<MeshUvAlign['axis'], number> = { '+v': 0, '-u': 1, '-v': 2, '+u': 3 };

export const DEFAULT_UV_ALIGN: MeshUvAlign = { up: '+y', fallback: '-z', axis: '+v' };

/** Trailing `unwrapUVs` wasm args: `[enabled, up xyz, fallback xyz, quarterTurns]`. */
export const uvAlignWasmArgs = (align: MeshUvAlign | undefined) => {
  const a = align ?? DEFAULT_UV_ALIGN;
  return [!!align, ...AXIS_VECS[a.up], ...AXIS_VECS[a.fallback], QUARTER_TURNS[a.axis]] as const;
};
