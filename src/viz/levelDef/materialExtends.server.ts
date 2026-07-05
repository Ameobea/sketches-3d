import type { MaterialDefRaw, MaterialExtendsRef, TextureDef } from './types';

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
 * Resolves a non-local (`library`/`geotoy`) extends parent to a fully-flattened `customShader` def,
 * merging any textures it pulls in into `textures`. Injected by the caller so this module needn't
 * import the library/geotoy resolvers (which would form an import cycle).
 */
export type ExternalParentResolver = (
  ref: Extract<MaterialExtendsRef, { type: 'library' | 'geotoy' }>,
  textures: Record<string, TextureDef>
) => Promise<MaterialDefRaw>;

/**
 * Resolves `extends` across a level's material registry into fully-flattened defs. A child
 * deep-merges over its parent; the parent is another material in the record (`local`), a shared
 * library material (`library`), or a Geotoy material (`geotoy`) — the latter two resolved via the
 * injected `resolveExternal`. `extends` is stripped from the result and the resolved parent must be
 * `customShader`. Throws on a missing local parent or an `extends` cycle.
 */
export const resolveMaterialExtends = async (
  materials: Record<string, MaterialDefRaw>,
  resolveExternal: ExternalParentResolver,
  textures: Record<string, TextureDef>
): Promise<Record<string, MaterialDefRaw>> => {
  const resolved = new Map<string, MaterialDefRaw>();
  const resolving = new Set<string>();

  const resolve = async (name: string, chain: string[]): Promise<MaterialDefRaw> => {
    const cached = resolved.get(name);
    if (cached) {
      return cached;
    }
    if (resolving.has(name)) {
      throw new Error(`[resolveMaterialExtends] cyclic \`extends\`: ${[...chain, name].join(' -> ')}`);
    }
    const def = materials[name];
    if (!def) {
      const from = chain.length ? ` (from "${chain[chain.length - 1]}")` : '';
      throw new Error(`[resolveMaterialExtends] \`extends\` references unknown material "${name}"${from}`);
    }
    if (def.type !== 'customShader' || def.extends === undefined) {
      resolved.set(name, def);
      return def;
    }

    const ext = def.extends;
    let parent: MaterialDefRaw;
    if (ext.type === 'local') {
      resolving.add(name);
      parent = await resolve(ext.name, [...chain, name]);
      resolving.delete(name);
    } else {
      parent = await resolveExternal(ext, textures);
    }
    if (parent.type !== 'customShader') {
      const desc =
        ext.type === 'local'
          ? `"${ext.name}"`
          : ext.type === 'library'
            ? `library "${ext.path}"`
            : `geotoy material ${ext.materialId}`;
      throw new Error(
        `[resolveMaterialExtends] "${name}" extends ${desc} of type "${parent.type}"; only customShader supports extends`
      );
    }

    const { extends: _drop, ...child } = def;
    const merged = deepMerge(parent, child) as MaterialDefRaw;
    resolved.set(name, merged);
    return merged;
  };

  const out: Record<string, MaterialDefRaw> = {};
  for (const name of Object.keys(materials)) {
    out[name] = await resolve(name, []);
  }
  return out;
};
