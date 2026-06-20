/** Categorical "curated neon" palette for gizmo handles, assigned per-node by scan order.
 *  Single source of truth shared by the CodeMirror badges and the viewport ghost markers. */
export const GIZMO_PALETTE: readonly string[] = [
  '#22d3ee', // cyan
  '#e84fd0', // magenta
  '#f5c518', // gold
  '#84cc16', // lime
  '#fb7233', // orange
  '#a78bfa', // violet
  '#2dd4bf', // teal
  '#fb5c8a', // rose
];

export const gizmoColorForIndex = (i: number): string => {
  const n = GIZMO_PALETTE.length;
  return GIZMO_PALETTE[((i % n) + n) % n];
};

const hexToRgb = (hex: string): [number, number, number] => {
  const h = hex.replace('#', '');
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
};

/** Mix a hex color toward its own luminance-gray. `amount`: 0 = unchanged, 1 = fully gray. */
export const desaturatedCss = (hex: string, amount: number, alpha = 1): string => {
  const [r, g, b] = hexToRgb(hex);
  const lum = 0.299 * r + 0.587 * g + 0.114 * b;
  const mix = (c: number) => Math.round(c + (lum - c) * amount);
  return `rgba(${mix(r)}, ${mix(g)}, ${mix(b)}, ${alpha})`;
};
