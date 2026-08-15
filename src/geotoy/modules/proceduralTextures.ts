import * as THREE from 'three';

import type { MaterialDef } from 'src/geoscript/materials';
import type { TreeDef } from 'src/geoscript/geotoyAPIClient';
import type { GeneratedTexture } from 'src/geoscript/runner/types';

/**
 * Material texture slots reference a texture tab's `render_texture` output through a
 * sentinel handle string, `procedural:<tabId>:<outputName>`, flowing through the same
 * string-handle plumbing as library texture ids. Each handle owns one stable
 * `DataTexture`: materials capture it at build time and every run's outputs are uploaded
 * into it in place, so material builds and texture evals never need ordering.
 *
 * Pixels are stored as linear (non-sRGB) 8-bit RGBA — the geoscript values themselves,
 * matching what the 2D preview shows before its sRGB display transform.
 */
export const PROCEDURAL_HANDLE_PREFIX = 'procedural:';

export const isProceduralHandle = (handle: string): boolean => handle.startsWith(PROCEDURAL_HANDLE_PREFIX);

export const buildProceduralHandle = (tabId: string, output: string): string =>
  `${PROCEDURAL_HANDLE_PREFIX}${tabId}:${output}`;

/** Output names are free-form and may contain `:`; tab ids can't, so split on the first. */
export const parseProceduralHandle = (handle: string): { tabId: string; output: string } | null => {
  if (!isProceduralHandle(handle)) return null;
  const rest = handle.slice(PROCEDURAL_HANDLE_PREFIX.length);
  const ix = rest.indexOf(':');
  return ix > 0 ? { tabId: rest.slice(0, ix), output: rest.slice(ix + 1) } : null;
};

const registry = new Map<string, THREE.DataTexture>();

const WRAP: Record<GeneratedTexture['wrap'], THREE.Wrapping> = {
  repeat: THREE.RepeatWrapping,
  clamp: THREE.ClampToEdgeWrapping,
  mirror: THREE.MirroredRepeatWrapping,
};

/** Stable per-handle texture; starts as a 1×1 mid-gray placeholder until a run uploads. */
export const getProceduralTexture = (handle: string): THREE.DataTexture => {
  let tex = registry.get(handle);
  if (!tex) {
    tex = new THREE.DataTexture(new Uint8Array([128, 128, 128, 255]), 1, 1);
    tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
    tex.magFilter = THREE.LinearFilter;
    tex.minFilter = THREE.LinearMipmapLinearFilter;
    tex.generateMipmaps = true;
    tex.needsUpdate = true;
    registry.set(handle, tex);
  }
  return tex;
};

/** In-place upload of a run's outputs into any matching placeholder textures. */
export const uploadProceduralTextures = (textures: GeneratedTexture[]) => {
  for (const t of textures) {
    const sep = t.sourceModule.indexOf(':');
    if (sep <= 0) continue;
    const tex = registry.get(buildProceduralHandle(t.sourceModule.slice(0, sep), t.name));
    if (!tex) continue;

    const n = t.width * t.height;
    const out = new Uint8Array(n * 4);
    const px = t.data;
    const c = t.channels;
    const to8 = (v: number) => (v <= 0 ? 0 : v >= 1 ? 255 : Math.round(v * 255));
    for (let i = 0; i < n; i += 1) {
      const b = i * c;
      out[i * 4] = to8(px[b]);
      out[i * 4 + 1] = to8(px[c >= 3 ? b + 1 : b]);
      out[i * 4 + 2] = to8(px[c >= 3 ? b + 2 : b]);
      out[i * 4 + 3] = c === 4 ? to8(px[b + 3]) : 255;
    }
    // GL storage is immutable (texStorage2D): a dimension change needs a fresh GL texture,
    // which dispose() forces while keeping this JS instance (and every material holding it).
    if (tex.image.width !== t.width || tex.image.height !== t.height) tex.dispose();
    tex.image = { data: out, width: t.width, height: t.height } as THREE.DataTexture['image'];
    tex.wrapS = tex.wrapT = WRAP[t.wrap];
    tex.needsUpdate = true;
  }
};

const TEXTURE_SLOTS = [
  'map',
  'normalMap',
  'roughnessMap',
  'metalnessMap',
  'clearcoatNormalMap',
  'pomHeightMap',
] as const;

export const proceduralHandlesForDef = (def: MaterialDef): string[] => {
  if (def.type !== 'customShader' || !def.props) return [];
  return TEXTURE_SLOTS.flatMap(slot => {
    const h = def.props?.[slot];
    return h != null && isProceduralHandle(h) ? [h] : [];
  });
};

export interface ProceduralTextureOption {
  handle: string;
  label: string;
  usage: string | null;
}

/** Static scan of texture tabs' node sources for `render_texture` output names + usage.
 *  Regex-level — misses dynamically-computed names; those still work as hand-typed refs. */
export const scanProceduralOutputs = (
  textureTabs: { id: string; tree: TreeDef }[]
): ProceduralTextureOption[] => {
  const out: ProceduralTextureOption[] = [];
  const seen = new Set<string>();
  for (const { id, tree } of textureTabs) {
    for (const node of Object.values(tree.nodes)) {
      if (node.disabled) continue;
      for (const m of node.source.matchAll(/render_texture\s*\(([^)]*)/g)) {
        const args = m[1];
        const name = args.match(/name\s*=\s*"([^"]+)"/)?.[1] ?? args.match(/^\s*"([^"]+)"/)?.[1];
        if (!name) continue;
        const handle = buildProceduralHandle(id, name);
        if (seen.has(handle)) continue;
        seen.add(handle);
        out.push({ handle, label: `${id}:${name}`, usage: args.match(/usage\s*=\s*"([^"]+)"/)?.[1] ?? null });
      }
    }
  }
  return out;
};

/** Dispose + drop registry textures no material references anymore; without this the
 *  GPU textures (and last-uploaded pixels) of removed refs leak for the session. */
export const pruneProceduralTextures = (defs: Record<string, MaterialDef>) => {
  const live = new Set(Object.values(defs).flatMap(proceduralHandlesForDef));
  for (const [handle, tex] of registry) {
    if (!live.has(handle)) {
      tex.dispose();
      registry.delete(handle);
    }
  }
};

/** Texture tabs the material set depends on — these join the run set as render deps. */
export const proceduralRefTabIds = (defs: Record<string, MaterialDef>): Set<string> => {
  const out = new Set<string>();
  for (const def of Object.values(defs)) {
    for (const h of proceduralHandlesForDef(def)) {
      const parsed = parseProceduralHandle(h);
      if (parsed) out.add(parsed.tabId);
    }
  }
  return out;
};
