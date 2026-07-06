import type {
  AssetDef,
  GeoscriptAssetDef,
  GeotoyCompositionAssetDef,
  InputValueJson,
  ObjectDef,
} from './types';

/** djb2 hash over a string, returned as a hex string. */
export const djb2Hash = (s: string): string => {
  let h = 5381;
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) + h + s.charCodeAt(i)) | 0;
  }
  return (h >>> 0).toString(16);
};

/** Deterministic JSON with sorted object keys, for hashing/equality of input maps. */
export const canonicalizeInputs = (v: unknown): string => {
  if (Array.isArray(v)) return `[${v.map(canonicalizeInputs).join(',')}]`;
  if (v && typeof v === 'object') {
    const entries = Object.keys(v as Record<string, unknown>)
      .sort()
      .map(k => `${JSON.stringify(k)}:${canonicalizeInputs((v as Record<string, unknown>)[k])}`);
    return `{${entries.join(',')}}`;
  }
  return JSON.stringify(v);
};

export type InputsJson = Record<string, InputValueJson>;

/**
 * Resolution-layer view of parametric placements. Objects carrying `inputs` resolve to synthetic
 * "variant" assets — the base asset def with the merged (asset ⊕ object) inputs — deduped by
 * canonical input equality. The authored `LevelDef` is never mutated: variants exist only in
 * `assets` here, and `ObjectDef.asset` keeps the authored id.
 */
export interface ParamVariants {
  /** Authored assets plus one synthesized def per distinct variant, keyed by variant id. */
  assets: Record<string, AssetDef>;
  /** Ids of the synthesized entries (used to skip `_meta` collection for variants). */
  variantIds: Set<string>;
  /** Base asset id → its variant ids. */
  variantsByBase: Map<string, string[]>;
  /** The asset id this placement resolves against (variant id, or the authored id). */
  effectiveAssetId(def: ObjectDef): string;
  /** Re-synthesize a variant def from an updated base def (geo hot-reload). */
  synthesize(base: AssetDef, variantId: string): AssetDef;
}

type ParametricAssetDef = GeoscriptAssetDef | GeotoyCompositionAssetDef;

const asParametric = (def: AssetDef | undefined): ParametricAssetDef | null =>
  def && (def.type === 'geoscript' || def.type === 'geotoyComposition') ? def : null;

export const variantAssetId = (baseId: string, mergedInputs: InputsJson): string =>
  `${baseId}@${djb2Hash(canonicalizeInputs(mergedInputs))}`;

export const expandParamVariants = (
  assets: Record<string, AssetDef>,
  leafDefs: ObjectDef[]
): ParamVariants => {
  const expanded: Record<string, AssetDef> = { ...assets };
  const variantIds = new Set<string>();
  const variantsByBase = new Map<string, string[]>();
  const mergedByVariant = new Map<string, InputsJson>();

  // Pure function of the def so it stays correct for defs created after load (paste/spawn).
  const mergedInputsFor = (def: ObjectDef): { base: ParametricAssetDef; merged: InputsJson } | null => {
    if (!def.asset || !def.inputs || Object.keys(def.inputs).length === 0) return null;
    const base = asParametric(assets[def.asset]);
    if (!base) {
      // Schema validation rejects this; guard for defs mutated at runtime.
      console.error(
        `[paramVariants] object "${def.id}" has inputs but asset "${def.asset}" is not parametric`
      );
      return null;
    }
    const merged: InputsJson = { ...(base.inputs ?? {}), ...def.inputs };
    return canonicalizeInputs(merged) === canonicalizeInputs(base.inputs ?? {}) ? null : { base, merged };
  };

  for (const obj of leafDefs) {
    const m = mergedInputsFor(obj);
    if (!m) continue;
    const vid = variantAssetId(obj.asset!, m.merged);
    if (variantIds.has(vid)) continue;
    if (assets[vid])
      throw new Error(`[paramVariants] variant id "${vid}" collides with an authored asset id`);
    expanded[vid] = { ...m.base, inputs: m.merged };
    variantIds.add(vid);
    mergedByVariant.set(vid, m.merged);
    const list = variantsByBase.get(obj.asset!) ?? [];
    list.push(vid);
    variantsByBase.set(obj.asset!, list);
  }

  return {
    assets: expanded,
    variantIds,
    variantsByBase,
    effectiveAssetId: def => {
      const m = mergedInputsFor(def);
      return m ? variantAssetId(def.asset!, m.merged) : (def.asset ?? '');
    },
    synthesize: (base, vid) => {
      const p = asParametric(base);
      return p ? { ...p, inputs: mergedByVariant.get(vid) } : base;
    },
  };
};
