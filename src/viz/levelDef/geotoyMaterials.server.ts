import { getGeotoyAPIBaseURL, getMaterial, getMultipleTextures } from 'src/geoscript/geotoyAPIClient';
import type { MaterialDef, TextureDef } from './types';

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

/**
 * Inlines a geotoy `customShader`/`customBasicShader` def that's already in hand: resolves its
 * texture-id handles to CDN URLs, registers them as synthesized level `textures` entries (slot-aware
 * colorSpace/format), and rewrites the handles to those keys. `ctx` labels errors. Synthesized keys
 * are content-addressed (`geotoyTextureKey`), so the same texture across materials dedupes.
 */
export const inlineGeotoyMaterialTextures = async (
  def: MaterialDef,
  synthesized: Record<string, TextureDef>,
  ctx: string
): Promise<MaterialDef> => {
  if (def.type === 'customBasicShader') return def;
  if (def.type !== 'customShader') {
    throw new Error(
      `[inlineGeotoyMaterialTextures] ${ctx} has unsupported stored type "${def.type}" — expected the unified customShader/customBasicShader shape`
    );
  }

  const props: Record<string, unknown> = { ...(def.props ?? {}) };
  const slotRefs: { slot: string; texId: number; cfg: Partial<TextureDef> }[] = [];
  for (const [slot, cfg] of Object.entries(GEOTOY_TEXTURE_SLOTS)) {
    const handle = props[slot];
    if (typeof handle === 'string' && handle !== '') slotRefs.push({ slot, texId: Number(handle), cfg });
  }
  if (slotRefs.length === 0) return def;

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
    synthesized[key] = {
      url: tex.url,
      magFilter: 'linear',
      minFilter: 'linearMipLinear',
      anisotropy: 16,
      ...cfg,
    };
    props[slot] = key;
  }
  return { ...def, props } as MaterialDef;
};

/**
 * Resolves a `geotoyMaterial` ref by fetching its def from the geotoy backend (by id) and inlining
 * its textures via {@link inlineGeotoyMaterialTextures}, so the client receives a fully level-native
 * material. Private materials resolve via `GEOTOY_ADMIN_TOKEN`.
 */
export const resolveGeotoyMaterial = async (
  materialId: number,
  synthesized: Record<string, TextureDef>,
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
