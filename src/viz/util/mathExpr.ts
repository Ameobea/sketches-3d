/** Evaluates expressions like `2*pi/4`, `(3+1)/8`. Returns null on parse failure
 *  or non-finite result. Regex whitelist gates `new Function` against arbitrary identifiers. */
const compiledCache = new Map<string, (pi: number) => unknown>();

export const evalMathExpr = (s: string): number | null => {
  const trimmed = s.trim();
  if (!trimmed) return null;
  const sanitized = trimmed.replace(/\bpi\b/gi, 'PI');
  if (!/^[\d\s+\-*/().PI]+$/.test(sanitized)) return null;
  let fn = compiledCache.get(sanitized);
  if (!fn) {
    try {
      fn = new Function('PI', `"use strict"; return (${sanitized});`) as (pi: number) => unknown;
    } catch {
      return null;
    }
    if (compiledCache.size >= 256) compiledCache.clear();
    compiledCache.set(sanitized, fn);
  }
  try {
    const result = fn(Math.PI);
    return typeof result === 'number' && isFinite(result) ? result : null;
  } catch {
    return null;
  }
};
