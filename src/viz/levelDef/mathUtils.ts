/**
 * Rounds a number to 4 decimal places.
 * Commonly used for serialization of transforms to avoid floating-point precision issues.
 */
export const round = (n: number): number => Math.round(n * 10000) / 10000;
