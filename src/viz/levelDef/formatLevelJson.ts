import { COLOR_KEYS, hexIntToStr } from './colorUtils';

const NUM = '-?(?:[0-9]+(?:\\.[0-9]*)?|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?';
const NUMERIC_ARRAY_RE = new RegExp(`\\[\\s*${NUM}(?:\\s*,\\s*${NUM})*\\s*\\]`, 'g');

const colorReplacer = (key: string, value: unknown): unknown => {
  if (COLOR_KEYS.has(key) && typeof value === 'number' && Number.isInteger(value)) {
    return hexIntToStr(value);
  }
  return value;
};

/**
 * Some custom formatting on top of default `JSON.stringify`:
 * - Collapses numeric arrays onto a single line
 * - Serializes color integer values as human-readable "#rrggbb" hex strings
 */
export const formatLevelJson = (obj: unknown): string => {
  const json = JSON.stringify(obj, colorReplacer, 2);
  const out = json.replace(NUMERIC_ARRAY_RE, match => {
    const nums = match
      .slice(1, -1)
      .trim()
      .split(/\s*,\s*/);
    return '[' + nums.join(', ') + ']';
  });
  return out + '\n';
};
