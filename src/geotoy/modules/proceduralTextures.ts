import * as THREE from 'three';

import { getDefaultAnisotropy, getDefaultMagFilter } from 'src/viz/conf';

import type { MaterialDef } from 'src/geoscript/materials';
import { TEXTURE_SLOTS } from 'src/viz/materials/schema';
import type { TextureOutputMeta } from 'src/geoscript/geotoyAPIClient';
import type { GeneratedTexture } from 'src/geoscript/runner/types';

/**
 * Each `procedural:` handle owns one stable `DataTexture`: materials capture it at build
 * time and every run's outputs are uploaded into it in place, so material builds and
 * texture evals never need ordering. Handle string format lives in
 * `proceduralHandleFormat.ts` (re-exported here) so server code can parse handles without
 * pulling in THREE/conf.
 *
 * Pixels are stored linear (non-sRGB) in the output's materialization format — `rgba8`
 * unless overridden per output (see `formatOptionsForChannels`). The values are the
 * geoscript f32s themselves, matching what the 2D preview shows before its sRGB display
 * transform; u8 formats quantize with a [0,1] clamp at upload.
 */
export {
  PROCEDURAL_HANDLE_PREFIX,
  PROCEDURAL_STACK_HANDLE_PREFIX,
  isProceduralHandle,
  isStackHandle,
  buildProceduralHandle,
  parseProceduralHandle,
} from './proceduralHandleFormat';
import {
  buildProceduralHandle,
  isProceduralHandle,
  isStackHandle,
  parseProceduralHandle,
} from './proceduralHandleFormat';

export { STACK_CAPABLE_SLOTS } from 'src/viz/materials/schema';

const registry = new Map<string, THREE.DataTexture | THREE.DataArrayTexture>();

const WRAP: Record<GeneratedTexture['wrap'], THREE.Wrapping> = {
  repeat: THREE.RepeatWrapping,
  clamp: THREE.ClampToEdgeWrapping,
  mirror: THREE.MirroredRepeatWrapping,
};

const FILTERS: Record<string, THREE.TextureFilter> = {
  nearest: THREE.NearestFilter,
  linear: THREE.LinearFilter,
  nearest_mipmap_nearest: THREE.NearestMipmapNearestFilter,
  nearest_mipmap_linear: THREE.NearestMipmapLinearFilter,
  linear_mipmap_nearest: THREE.LinearMipmapNearestFilter,
  linear_mipmap_linear: THREE.LinearMipmapLinearFilter,
};

/** Unset filters fall back to the app-wide `loadTexture` defaults. */
export const DEFAULT_MIN_FILTER = 'nearest_mipmap_linear';
export const defaultMagFilter = (): string => getDefaultMagFilter();
export const DEFAULT_FORMAT = 'rgba8';

/** Format options valid for a given synthesis channel count, default first. `rgba8`
 *  replicates 1-channel gray into RGB (so grayscale works in color slots) and zero-fills a
 *  missing B; tight/float variants upload exactly the channels named. */
export const formatOptionsForChannels = (channels: number): string[] => {
  switch (channels) {
    case 1:
      return ['rgba8', 'r8', 'r32f'];
    case 2:
      return ['rgba8', 'rg8', 'rg32f'];
    default:
      return ['rgba8', 'rgba32f'];
  }
};

/** Float targets can't have mipmaps generated for them; strip the mip stage. */
const clampMinFilterForFloat = (name: string): string =>
  name.includes('mipmap') ? name.split('_')[0] : name;

/** Stable per-handle texture; starts as a mid-gray placeholder (1×1, or 1×1×2 for stack
 *  handles) until a run uploads. */
export const getProceduralTexture = (handle: string): THREE.DataTexture | THREE.DataArrayTexture => {
  let tex = registry.get(handle);
  if (!tex) {
    const gray = (n: number) => {
      const px = new Uint8Array(n * 4);
      for (let i = 0; i < n; i += 1) px.set([128, 128, 128, 255], i * 4);
      return px;
    };
    tex = isStackHandle(handle)
      ? new THREE.DataArrayTexture(gray(2), 1, 1, 2)
      : new THREE.DataTexture(gray(1), 1, 1);
    tex.wrapS = tex.wrapT = THREE.RepeatWrapping;
    tex.magFilter = FILTERS[defaultMagFilter()] as THREE.MagnificationTextureFilter;
    tex.minFilter = FILTERS[DEFAULT_MIN_FILTER];
    tex.generateMipmaps = true;
    tex.anisotropy = getDefaultAnisotropy();
    tex.needsUpdate = true;
    registry.set(handle, tex);
  }
  return tex;
};

const to8 = (v: number) => (v <= 0 ? 0 : v >= 1 ? 255 : Math.round(v * 255));

/** Encode a run output's f32 pixels for its materialization format. u8 formats normally
 *  arrive pre-encoded from the worker (`t.encoded`, SIMD in wasm) and rgba32f 3ch as
 *  `t.rgba`; the JS loops are the fallback. Per-pixel, so concatenated stack slices encode
 *  in one pass. */
const encodePixels = (
  t: GeneratedTexture,
  format: string
): { data: Uint8Array | Float32Array; glFormat: THREE.PixelFormat; type: THREE.TextureDataType } => {
  if (t.encoded && t.encodedFormat === format) {
    const glFormat = format === 'r8' ? THREE.RedFormat : format === 'rg8' ? THREE.RGFormat : THREE.RGBAFormat;
    return { data: t.encoded, glFormat, type: THREE.UnsignedByteType };
  }
  const n = t.width * t.height * t.layers;
  const c = t.channels;
  if (c === 3 && format === 'rgba32f' && t.rgba) {
    return { data: t.rgba, glFormat: THREE.RGBAFormat, type: THREE.FloatType };
  }
  if (!t.data) throw new Error(`texture "${t.name}": no raw pixels for format ${format}`);
  const px = t.data;
  switch (format) {
    case 'r8': {
      const out = new Uint8Array(n);
      for (let i = 0; i < n; i += 1) out[i] = to8(px[i * c]);
      return { data: out, glFormat: THREE.RedFormat, type: THREE.UnsignedByteType };
    }
    case 'rg8': {
      const out = new Uint8Array(n * 2);
      for (let i = 0; i < n; i += 1) {
        out[i * 2] = to8(px[i * c]);
        out[i * 2 + 1] = to8(px[i * c + 1]);
      }
      return { data: out, glFormat: THREE.RGFormat, type: THREE.UnsignedByteType };
    }
    case 'r32f': {
      const out = c === 1 ? px : new Float32Array(n).map((_, i) => px[i * c]);
      return { data: out, glFormat: THREE.RedFormat, type: THREE.FloatType };
    }
    case 'rg32f': {
      let out = px;
      if (c !== 2) {
        out = new Float32Array(n * 2);
        for (let i = 0; i < n; i += 1) {
          out[i * 2] = px[i * c];
          out[i * 2 + 1] = px[i * c + 1];
        }
      }
      return { data: out, glFormat: THREE.RGFormat, type: THREE.FloatType };
    }
    case 'rgba32f': {
      let out = c === 4 ? px : t.rgba;
      if (!out) {
        out = new Float32Array(n * 4);
        for (let i = 0; i < n; i += 1) {
          const b = i * c;
          out[i * 4] = px[b];
          out[i * 4 + 1] = c >= 2 ? px[b + 1] : px[b];
          out[i * 4 + 2] = c >= 3 ? px[b + 2] : c === 1 ? px[b] : 0;
          out[i * 4 + 3] = 1;
        }
      }
      return { data: out, glFormat: THREE.RGBAFormat, type: THREE.FloatType };
    }
    // rgba8
    default: {
      const out = new Uint8Array(n * 4);
      for (let i = 0; i < n; i += 1) {
        const b = i * c;
        out[i * 4] = to8(px[b]);
        out[i * 4 + 1] = to8(c >= 2 ? px[b + 1] : px[b]);
        out[i * 4 + 2] = to8(c >= 3 ? px[b + 2] : c === 1 ? px[b] : 0);
        out[i * 4 + 3] = c === 4 ? to8(px[b + 3]) : 255;
      }
      return { data: out, glFormat: THREE.RGBAFormat, type: THREE.UnsignedByteType };
    }
  }
};

const applyGeneratedTexture = (tex: THREE.DataTexture | THREE.DataArrayTexture, t: GeneratedTexture) => {
  const isStack = t.layers > 1;
  const format = t.format ?? DEFAULT_FORMAT;
  const isFloat = format.endsWith('32f');
  let minName = t.minFilter ?? DEFAULT_MIN_FILTER;
  if (isFloat) minName = clampMinFilterForFloat(minName);
  const genMips = !isFloat && minName.includes('mipmap');
  const { data, glFormat, type } = encodePixels(t, format);

  // GL storage is immutable (texStorage2D/3D): a dimension/format/mip-count change needs
  // a fresh GL texture, which dispose() forces while keeping this JS instance (and every
  // material holding it).
  if (
    tex.image.width !== t.width ||
    tex.image.height !== t.height ||
    (isStack && (tex.image as THREE.DataArrayTexture['image']).depth !== t.layers) ||
    tex.format !== glFormat ||
    tex.type !== type ||
    tex.generateMipmaps !== genMips
  ) {
    tex.dispose();
  }
  tex.image = (
    isStack
      ? { data, width: t.width, height: t.height, depth: t.layers }
      : { data, width: t.width, height: t.height }
  ) as THREE.DataTexture['image'];
  tex.format = glFormat;
  tex.type = type;
  tex.generateMipmaps = genMips;
  // Tightly-packed u8 rows aren't 4-byte multiples in general.
  tex.unpackAlignment = format === 'r8' || format === 'rg8' ? 1 : 4;
  tex.minFilter = FILTERS[minName] ?? FILTERS[DEFAULT_MIN_FILTER];
  tex.magFilter = (FILTERS[t.magFilter ?? defaultMagFilter()] ??
    FILTERS[defaultMagFilter()]) as THREE.MagnificationTextureFilter;
  tex.wrapS = tex.wrapT = WRAP[t.wrap];
  tex.anisotropy = getDefaultAnisotropy();
  tex.needsUpdate = true;
};

/** In-place upload of a run's outputs into any matching placeholder textures. Stack
 *  outputs only match stack-handle entries (and vice versa) since the handle key encodes
 *  stack-ness — a kind change under the same output name needs a re-pick, not an upload. */
export const uploadProceduralTextures = (textures: GeneratedTexture[]) => {
  for (const t of textures) {
    const sep = t.sourceModule.indexOf(':');
    if (sep <= 0) continue;
    const tex = registry.get(buildProceduralHandle(t.sourceModule.slice(0, sep), t.name, t.layers > 1));
    if (tex) applyGeneratedTexture(tex, t);
  }
};

/** Fresh standalone texture for a run output (level-def consumption); identical config to the
 *  registry upload path so level renders match Geotoy exactly. Pixels stay linear (non-sRGB). */
export const createGeneratedTexture = (t: GeneratedTexture): THREE.DataTexture | THREE.DataArrayTexture => {
  const tex = t.layers > 1 ? new THREE.DataArrayTexture() : new THREE.DataTexture();
  applyGeneratedTexture(tex, t);
  return tex;
};

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
  /** Slice count for stack outputs; undefined for singles. */
  layers?: number;
}

/** Picker options from each texture tab's last-known outputs (indexed from runs, persisted
 *  in tab metadata so never-yet-run tabs still list after a fresh load). */
export const proceduralOutputOptions = (
  tabs: readonly { id: string; name: string; kind: string; textureOutputs: readonly TextureOutputMeta[] }[]
): ProceduralTextureOption[] =>
  tabs
    .filter(t => t.kind === 'texture')
    .flatMap(t =>
      t.textureOutputs.map(o => ({
        handle: buildProceduralHandle(t.id, o.name, (o.layers ?? 1) > 1),
        label: `${t.name}:${o.name}`,
        usage: o.usage ?? null,
        layers: o.layers,
      }))
    );

/** Group a run's texture outputs by source tab, deduped by output name. */
export const textureOutputsByTab = (textures: GeneratedTexture[]): Map<string, TextureOutputMeta[]> => {
  const byTab = new Map<string, TextureOutputMeta[]>();
  for (const t of textures) {
    const sep = t.sourceModule.indexOf(':');
    if (sep <= 0) continue;
    const tabId = t.sourceModule.slice(0, sep);
    let list = byTab.get(tabId);
    if (!list) byTab.set(tabId, (list = []));
    if (!list.some(o => o.name === t.name)) {
      list.push({ name: t.name, usage: t.usage ?? undefined, layers: t.layers > 1 ? t.layers : undefined });
    }
  }
  return byTab;
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
