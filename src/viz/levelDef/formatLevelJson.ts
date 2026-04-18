const NUM = '-?(?:[0-9]+(?:\\.[0-9]*)?|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?';
const NUMERIC_ARRAY_RE = new RegExp(`\\[\\s*${NUM}(?:\\s*,\\s*${NUM})*\\s*\\]`, 'g');

// _meta contains no nested objects so [^{}]* safely matches the full block.
const META_BLOCK_RE = /"_meta":\s*\{[^{}]*\}/g;

/** JSON keys whose integer values represent hex colors and should be serialized as "#rrggbb" strings. */
const COLOR_KEYS = new Set(['color', 'sheenColor']);

const colorReplacer = (key: string, value: unknown): unknown => {
  if (COLOR_KEYS.has(key) && typeof value === 'number' && Number.isInteger(value)) {
    return '#' + value.toString(16).padStart(6, '0');
  }
  return value;
};

/**
 * Some custom formatting on top of default `JSON.stringify`:
 * - Collapses numeric arrays onto a single line
 * - Serializes color integer values as human-readable "#rrggbb" hex strings
 * - Collapses `"_meta": { ... }` objects onto a single line
 */
export const formatLevelJson = (obj: unknown): string => {
  const json = JSON.stringify(obj, colorReplacer, 2);
  let out = json.replace(NUMERIC_ARRAY_RE, match => {
    const nums = match
      .slice(1, -1)
      .trim()
      .split(/\s*,\s*/);
    return '[' + nums.join(', ') + ']';
  });
  out = out.replace(META_BLOCK_RE, match => match.replace(/\s+/g, ' '));
  return out + '\n';
};
