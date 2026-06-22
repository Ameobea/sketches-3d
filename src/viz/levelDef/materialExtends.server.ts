import type { MaterialDefRaw } from './types';

const isPlainObject = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v);

// Override-merge for material defs: plain objects merge per-key (props, options.pom,
// shaders.customUniforms, …); arrays and scalars replace wholesale.
const deepMerge = (base: unknown, over: unknown): unknown => {
  if (isPlainObject(base) && isPlainObject(over)) {
    const out: Record<string, unknown> = { ...base };
    for (const [k, v] of Object.entries(over)) {
      out[k] = k in base ? deepMerge(base[k], v) : v;
    }
    return out;
  }
  return over;
};

/**
 * Resolve `extends` chains in a level's material registry into fully-flattened defs. A child
 * deep-merges over its parent (a level-local name or an already-pulled-in `__ASSETS__/…` library
 * material — both are keys here by the time this runs). `extends` is stripped from the result;
 * parent and child must both be `customShader`. Throws on a missing parent or an `extends` cycle.
 */
export const flattenMaterialExtends = (
  materials: Record<string, MaterialDefRaw>
): Record<string, MaterialDefRaw> => {
  const resolved = new Map<string, MaterialDefRaw>();
  const resolving = new Set<string>();

  const resolve = (name: string, chain: string[]): MaterialDefRaw => {
    const cached = resolved.get(name);
    if (cached) {
      return cached;
    }
    if (resolving.has(name)) {
      throw new Error(`[flattenMaterialExtends] cyclic \`extends\`: ${[...chain, name].join(' -> ')}`);
    }
    const def = materials[name];
    if (!def) {
      const from = chain.length ? ` (from "${chain[chain.length - 1]}")` : '';
      throw new Error(`[flattenMaterialExtends] \`extends\` references unknown material "${name}"${from}`);
    }
    const parentName = def.type === 'customShader' ? def.extends : undefined;
    if (parentName === undefined) {
      resolved.set(name, def);
      return def;
    }

    resolving.add(name);
    const parent = resolve(parentName, [...chain, name]);
    resolving.delete(name);
    if (parent.type !== 'customShader') {
      throw new Error(
        `[flattenMaterialExtends] "${name}" extends "${parentName}" of type "${parent.type}"; only customShader supports extends`
      );
    }

    const { extends: _drop, ...child } = def;
    const merged = deepMerge(parent, child) as MaterialDefRaw;
    resolved.set(name, merged);
    return merged;
  };

  const out: Record<string, MaterialDefRaw> = {};
  for (const name of Object.keys(materials)) {
    out[name] = resolve(name, []);
  }
  return out;
};
