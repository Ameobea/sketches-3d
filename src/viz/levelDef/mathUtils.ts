/** Round to 4 decimal places — used when serializing transforms to avoid float noise. */
export const round = (n: number): number => Math.round(n * 10000) / 10000;

/** Format a number for display: up to 4 decimals, trailing zeros trimmed (2 → "2.0", 1.5 → "1.5"). */
export const fmt = (n: number): string => {
  const s = n.toFixed(4);
  return s.replace(/(\.\d*?)0+$/, '$1').replace(/\.$/, '.0');
};
