/** Per-channel value statistics of a texture, as computed in wasm (`TexStats::to_wire`). */
export interface ChannelStats {
  min: number;
  max: number;
  mean: number;
  std: number;
  /** NaN/±inf texel count; excluded from every other field. */
  nonfinite: number;
  /** 257 points: `quantiles[i]` is the value at quantile i/256, so [0] = min and [256] = max. */
  quantiles: Float32Array;
}

export const WIRE_QUANTILES = 257;
const STRIDE = 5 + WIRE_QUANTILES;

export const parseChannelStats = (flat: ArrayLike<number>): ChannelStats[] => {
  const out: ChannelStats[] = [];
  for (let o = 0; o + STRIDE <= flat.length; o += STRIDE) {
    out.push({
      min: flat[o],
      max: flat[o + 1],
      mean: flat[o + 2],
      std: flat[o + 3],
      nonfinite: flat[o + 4],
      quantiles: Float32Array.from({ length: WIRE_QUANTILES }, (_, i) => flat[o + 5 + i]),
    });
  }
  return out;
};

/**
 * Mass histogram over `[lo, hi]` derived from a quantile table: each quantile step carries
 * `1 / steps` of the pixels, spread uniformly over the values it spans (a zero-width step is
 * a spike). Bins sum to the fraction of pixels inside the window.
 */
export const histogramFromQuantiles = (
  q: Float32Array,
  lo: number,
  hi: number,
  bins: number
): Float32Array => {
  const out = new Float32Array(bins);
  if (!(hi > lo) || q.length < 2) return out;
  const scale = bins / (hi - lo);
  const mass = 1 / (q.length - 1);
  const binOf = (x: number) => Math.min(bins - 1, Math.floor((x - lo) * scale));
  for (let i = 0; i + 1 < q.length; i++) {
    const a = q[i];
    const b = q[i + 1];
    if (b <= a) {
      if (a >= lo && a <= hi) out[binOf(a)] += mass;
      continue;
    }
    const x0 = Math.max(a, lo);
    const x1 = Math.min(b, hi);
    if (x1 <= x0) continue;
    const density = mass / (b - a);
    for (let bi = binOf(x0); bi <= binOf(x1); bi++) {
      const s = lo + bi / scale;
      const e = lo + (bi + 1) / scale;
      const overlap = Math.min(e, x1) - Math.max(s, x0);
      if (overlap > 0) out[bi] += density * overlap;
    }
  }
  return out;
};

/** Widens a histogram window by `frac` per side so spikes at its bounds (masks, clipped
 *  tails) draw inside the canvas rather than on its edge. */
export const padWindow = (lo: number, hi: number, frac = 0.03): [number, number] => {
  const pad = (hi - lo) * frac;
  return [lo - pad, hi + pad];
};
