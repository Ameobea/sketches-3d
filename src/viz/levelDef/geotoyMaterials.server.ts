import { getGeotoyAPIBaseURL, getMaterial, getMultipleTextures } from 'src/geoscript/geotoyAPIClient';
import type { MaterialDef, TextureDef } from './types';

/** Texture-bearing slots of a customShader def + their sampler semantics. Geotoy handles in these
 *  slots are texture ids; everything else (base color is the only sRGB slot) samples linearly. */
const GEOTOY_TEXTURE_SLOTS: Record<string, Partial<TextureDef>> = {
  map: { colorSpace: 'srgb' },
  normalMap: {},
  roughnessMap: {},
  metalnessMap: {},
  clearcoatNormalMap: {},
  pomHeightMap: { format: 'red' },
};

const geotoyTextureKey = (texId: number, cfg: Partial<TextureDef>): string =>
  `__geotoy__/${texId}${cfg.colorSpace === 'srgb' ? '/srgb' : ''}${cfg.format ? `/${cfg.format}` : ''}`;

/**
 * Resolves a `geotoyMaterial` ref by fetching its def from the geotoy backend and inlining it, so
 * the client receives a fully level-native material. Geotoy texture-id handles are resolved to CDN
 * URLs and registered as synthesized level `textures` entries (slot-aware colorSpace/format); the
 * material's handles are rewritten to those keys. Private materials resolve via `GEOTOY_ADMIN_TOKEN`.
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
  const storedType: string = def.type;
  if (storedType === 'customBasicShader') return def;
  if (storedType !== 'customShader') {
    throw new Error(
      `[resolveGeotoyMaterial] ${ctx} has unsupported stored type "${storedType}" — expected the unified customShader/customBasicShader shape`
    );
  }

  const props: Record<string, unknown> = { ...(def.props ?? {}) };
  const slotRefs: { slot: string; texId: number; cfg: Partial<TextureDef> }[] = [];
  for (const [slot, cfg] of Object.entries(GEOTOY_TEXTURE_SLOTS)) {
    const handle = props[slot];
    if (typeof handle === 'string' && handle !== '') slotRefs.push({ slot, texId: Number(handle), cfg });
  }
  if (slotRefs.length === 0) return def;

  let descriptors;
  try {
    const ids = [...new Set(slotRefs.map(r => r.texId))];
    descriptors = await getMultipleTextures(ids, globalThis.fetch, adminToken, baseUrl);
  } catch (err) {
    throw new Error(
      `[resolveGeotoyMaterial] ${ctx} texture resolution failed: ${err instanceof Error ? err.message : String(err)}`
    );
  }
  const byId = new Map(descriptors.map(d => [d.id, d]));
  for (const { slot, texId, cfg } of slotRefs) {
    const tex = byId.get(texId);
    if (!tex) {
      throw new Error(
        `[resolveGeotoyMaterial] ${ctx} references missing texture id ${texId} (slot "${slot}")`
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
