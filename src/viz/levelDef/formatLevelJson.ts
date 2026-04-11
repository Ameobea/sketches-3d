const NUM = '-?(?:[0-9]+(?:\\.[0-9]*)?|\\.[0-9]+)(?:[eE][+-]?[0-9]+)?';
const NUMERIC_ARRAY_RE = new RegExp(`\\[\\s*${NUM}(?:\\s*,\\s*${NUM})*\\s*\\]`, 'g');

/** Some custom formatting on top of default `JSON.stringify` to collapse numeric arrays onto a single line */
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
