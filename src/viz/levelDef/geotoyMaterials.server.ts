import { getGeotoyAPIBaseURL, getMaterial, getMultipleTextures } from 'src/geoscript/geotoyAPIClient';
import { parseProceduralHandle } from 'src/geotoy/modules/proceduralHandleFormat';
import type { AnyLevelTextureDef, MaterialDef, TextureDef } from './types';

/** Texture-bearing slots of a customShader def + their sampler semantics. Geotoy handles in these
 *  slots are texture ids; everything else (base color is the only sRGB slot) samples linearly. */
const GEOTOY_TEXTURE_SLOTS: Record<string, Partial<TextureDef>> = {
  map: { colorSpace: 'srgb' },
  normalMap: {},
  roughnessMap: {},
  metalnessMap: {},
  lightMap: {},
  transmissionMap: {},
  clearcoatNormalMap: {},
  pomHeightMap: { format: 'red' },
};

const geotoyTextureKey = (texId: number, cfg: Partial<TextureDef>): string =>
  `__geotoy__/${texId}${cfg.colorSpace === 'srgb' ? '/srgb' : ''}${cfg.format ? `/${cfg.format}` : ''}`;

/** Synthesized-texture key for a composition asset's procedural (run-produced) texture output.
 *  Stack-ness is part of the key, mirroring the geotoy registry: single and stack handles for
 *  the same output must not collide on one entry. */
export const procTextureKey = (assetId: string, tab: string, output: string, stack: boolean): string =>
  `__geotoyProc__/${assetId}/${tab}/${output}${stack ? '/stack' : ''}`;

/** Composition context for resolving `procedural:` handles in palette materials. */
export interface ProceduralInlineCtx {
  assetId: string;
  /** Texture tab ids present in the composition. */
  compTabIds: ReadonlySet<string>;
  /** Out-param: texture tabs referenced by inlined materials — render deps of the bake run. */
  refTabIds: Set<string>;
}

/**
 * Inlines a geotoy `customShader`/`customBasicShader` def that's already in hand: resolves its
 * texture-id handles to CDN URLs, registers them as synthesized level `textures` entries (slot-aware
 * colorSpace/format), and rewrites the handles to those keys. `ctx` labels errors. Synthesized keys
 * are content-addressed (`geotoyTextureKey`), so the same texture across materials dedupes.
 *
 * `procedural:<tab>:<output>` handles resolve against `procedural` (composition palette imports):
 * they synthesize `geotoyProcedural` texture entries bound to the owning asset's bake run. Without
 * a composition context (library materials by id) they're dropped with a warning.
 */
export const inlineGeotoyMaterialTextures = async (
  def: MaterialDef,
  synthesized: Record<string, AnyLevelTextureDef>,
  ctx: string,
  procedural?: ProceduralInlineCtx
): Promise<MaterialDef> => {
  if (def.type === 'customBasicShader') return def;
  if (def.type !== 'customShader') {
    throw new Error(
      `[inlineGeotoyMaterialTextures] ${ctx} has unsupported stored type "${def.type}" — expected the unified customShader/customBasicShader shape`
    );
  }

  const props: Record<string, unknown> = { ...(def.props ?? {}) };
  const slotRefs: { slot: string; texId: number; cfg: Partial<TextureDef> }[] = [];
  const procRefs: { slot: string; tabId: string; output: string; stack: boolean }[] = [];
  let droppedHandles = false;
  for (const [slot, cfg] of Object.entries(GEOTOY_TEXTURE_SLOTS)) {
    const handle = props[slot];
    if (typeof handle === 'string' && handle !== '') {
      if (!Number.isFinite(Number(handle))) {
        const parsed = parseProceduralHandle(handle);
        if (procedural && parsed && procedural.compTabIds.has(parsed.tabId)) {
          procRefs.push({ slot, ...parsed });
        } else {
          console.warn(
            `[inlineGeotoyMaterialTextures] ${ctx}: dropping unresolvable handle "${handle}" (slot "${slot}")`
          );
          delete props[slot];
          droppedHandles = true;
        }
        continue;
      }
      slotRefs.push({ slot, texId: Number(handle), cfg });
    }
  }

  // sampler2D custom uniforms hold direct URLs in geotoy-format defs; register each as a level
  // texture keyed by the URL itself (config matching geotoy's `loadUrlTexture`) so the loader's
  // pending-set gating resolves them instead of waiting forever on an unknown key.
  for (const u of Object.values(def.shaders?.customUniforms ?? {})) {
    if (u.type === 'sampler2D' && /^https?:\/\//.test(u.value)) {
      synthesized[u.value] = { url: u.value, magFilter: 'linear', minFilter: 'linearMipLinear' };
    }
  }

  if (slotRefs.length > 0) {
    const adminToken = process.env.GEOTOY_ADMIN_TOKEN || undefined;
    const baseUrl = getGeotoyAPIBaseURL();
    let descriptors;
    try {
      const ids = [...new Set(slotRefs.map(r => r.texId))];
      descriptors = await getMultipleTextures(ids, globalThis.fetch, adminToken, baseUrl);
    } catch (err) {
      throw new Error(
        `[inlineGeotoyMaterialTextures] ${ctx} texture resolution failed: ${err instanceof Error ? err.message : String(err)}`
      );
    }
    const byId = new Map(descriptors.map(d => [d.id, d]));
    for (const { slot, texId, cfg } of slotRefs) {
      const tex = byId.get(texId);
      if (!tex) {
        throw new Error(
          `[inlineGeotoyMaterialTextures] ${ctx} references missing texture id ${texId} (slot "${slot}")`
        );
      }
      const key = geotoyTextureKey(texId, cfg);
      synthesized[key] = { url: tex.url, ...cfg };
      props[slot] = key;
    }
  }

  // Applied after the fetch so a throw above leaves `synthesized`/`refTabIds` untouched —
  // a dropped material must not widen the run set or orphan texture entries.
  for (const { slot, tabId, output, stack } of procRefs) {
    const key = procTextureKey(procedural!.assetId, tabId, output, stack);
    synthesized[key] = { kind: 'geotoyProcedural', asset: procedural!.assetId, tab: tabId, output, stack };
    props[slot] = key;
    procedural!.refTabIds.add(tabId);
  }

  return slotRefs.length || procRefs.length || droppedHandles ? ({ ...def, props } as MaterialDef) : def;
};

/**
 * Resolves a `geotoyMaterial` ref by fetching its def from the geotoy backend (by id) and inlining
 * its textures via {@link inlineGeotoyMaterialTextures}, so the client receives a fully level-native
 * material. Private materials resolve via `GEOTOY_ADMIN_TOKEN`.
 */
export const resolveGeotoyMaterial = async (
  materialId: number,
  synthesized: Record<string, AnyLevelTextureDef>,
  label?: string
): Promise<MaterialDef> => {
  const ctx = `material ${materialId}${label ? ` ("${label}")` : ''}`;
  const adminToken = process.env.GEOTOY_ADMIN_TOKEN || undefined;
  const baseUrl = getGeotoyAPIBaseURL();
  let def: MaterialDef;
  try {
    def = (await getMaterial(materialId, globalThis.fetch, adminToken, baseUrl)).materialDefinition;
  } catch (err) {
    throw new Error(
      `[resolveGeotoyMaterial] Failed to resolve ${ctx}: ${err instanceof Error ? err.message : String(err)}`
    );
  }
  return inlineGeotoyMaterialTextures(def, synthesized, ctx);
};
