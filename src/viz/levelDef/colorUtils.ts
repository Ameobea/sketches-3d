/** Format a 0xRRGGBB integer as a "#rrggbb" CSS hex string. */
export const hexIntToStr = (n: number): string => '#' + (n >>> 0).toString(16).padStart(6, '0');

/** Parse a "#rrggbb" string to a 0xRRGGBB integer. */
export const hexStrToInt = (s: string): number => parseInt(s.slice(1), 16);

/** JSON keys whose values are 0xRRGGBB color integers (serialized to/from "#rrggbb" on disk). */
export const COLOR_KEYS = new Set(['color', 'sheenColor', 'skyColor', 'groundColor', 'horizonColor']);
