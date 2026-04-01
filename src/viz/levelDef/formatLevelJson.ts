/**
 * Serializes a level definition object to a compact-but-readable JSON string.
 *
 * Standard JSON.stringify with 2-space indent is used as the base, then a
 * post-processing pass collapses arrays whose elements are all numbers onto a
 * single line.  This keeps position/rotation/scale/uvScale readable without
 * sacrificing the structural indentation that makes the rest of the file easy
 * to navigate.
 *
 * Example — before:
 *   "position": [
 *     0.1234,
 *     -5.678,
 *     0
 *   ],
 *
 * After:
 *   "position": [0.1234, -5.678, 0],
 */

// Matches any JSON array whose contents are exclusively JSON numbers (including
// negatives, decimals, and scientific notation), possibly separated by whitespace
// and newlines (as produced by JSON.stringify with indentation).
const NUM = '-?(?:[0-9]+(?:\\.[0-9]*)?|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?';
const NUMERIC_ARRAY_RE = new RegExp(`\\[\\s*${NUM}(?:\\s*,\\s*${NUM})*\\s*\\]`, 'g');

export const formatLevelJson = (obj: unknown): string => {
  const json = JSON.stringify(obj, null, 2);
  return (
    json.replace(NUMERIC_ARRAY_RE, match => {
      const nums = match
        .slice(1, -1)
        .trim()
        .split(/\s*,\s*/);
      return '[' + nums.join(', ') + ']';
    }) + '\n'
  );
};
