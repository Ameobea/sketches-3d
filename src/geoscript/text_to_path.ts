import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import * as PathTessWasm from 'src/viz/wasmComp/path_tessellate';
import PathTessWasmURL from 'src/viz/wasmComp/path_tessellate_bg.wasm?url';

const PathTess = new AsyncOnce(async () => {
  await PathTessWasm.default(new URL(PathTessWasmURL, globalThis.location.href));
  return PathTessWasm;
});

interface TextToPathParams {
  fontFamily: string;
  fontSize?: number;
  fontWeight?: number | string;
  fontStyle?: 'normal' | 'italic' | 'oblique';
  letterSpacing?: number;
}

const DEFAULT_TEXT_TO_PATH_PARAMS: TextToPathParams = {
  fontFamily: 'IBM Plex Sans',
  fontSize: 72,
  fontStyle: 'normal',
};

const CachedTextToPathResults = new Map<
  string,
  { type: 'ok'; verts: Float32Array; indices: Uint32Array } | { type: 'err'; message: string }
>();

const buildCacheKey = (
  text: string,
  size: { width: number | null; height: number | null } | null,
  params: TextToPathParams
): string =>
  [
    text,
    size?.width || 0,
    size?.height || 0,
    params.fontFamily,
    params.fontSize || 0,
    params.fontWeight || '',
    params.fontStyle || '',
    params.letterSpacing || 0,
  ]
    .map(x => `${x}`)
    .join('|');

export const get_cached_text_to_path_verts = (
  text: string,
  fontFamily: string,
  fontSize: number,
  fontWeight: string,
  fontStyle: 'normal' | 'italic' | 'oblique',
  letterSpacing: number,
  width: number,
  height: number
): Float32Array | null => {
  const fontWeightNum = isNaN(Number(fontWeight)) ? fontWeight : Number(fontWeight);

  const cacheKey = buildCacheKey(
    text,
    { width: width ? width : null, height: height ? height : null },
    {
      fontFamily,
      fontSize: fontSize || undefined,
      fontWeight: fontWeight ? fontWeightNum : undefined,
      fontStyle: fontStyle ? fontStyle : undefined,
      letterSpacing: letterSpacing || undefined,
    }
  );

  const cached = CachedTextToPathResults.get(cacheKey);
  if (cached && cached.type === 'ok') {
    return cached.verts;
  }
  return null;
};

export const get_cached_text_to_path_indices = (
  text: string,
  fontFamily: string,
  fontSize: number,
  fontWeight: string,
  fontStyle: 'normal' | 'italic' | 'oblique',
  letterSpacing: number,
  width: number,
  height: number
): Uint32Array | null => {
  const fontWeightNum = isNaN(Number(fontWeight)) ? fontWeight : Number(fontWeight);

  const cacheKey = buildCacheKey(
    text,
    { width: width ? width : null, height: height ? height : null },
    {
      fontFamily,
      fontSize: fontSize || undefined,
      fontWeight: fontWeight ? fontWeightNum : undefined,
      fontStyle: fontStyle ? fontStyle : undefined,
      letterSpacing: letterSpacing || undefined,
    }
  );

  const cached = CachedTextToPathResults.get(cacheKey);
  if (cached && cached.type === 'ok') {
    return cached.indices;
  }
  return null;
};

export const get_cached_text_to_path_err = (
  text: string,
  fontFamily: string,
  fontSize: number,
  fontWeight: string,
  fontStyle: 'normal' | 'italic' | 'oblique',
  letterSpacing: number,
  width: number,
  height: number
): string | null => {
  const fontWeightNum = isNaN(Number(fontWeight)) ? fontWeight : Number(fontWeight);

  const cacheKey = buildCacheKey(
    text,
    { width: width ? width : null, height: height ? height : null },
    {
      fontFamily,
      fontSize: fontSize || undefined,
      fontWeight: fontWeight ? fontWeightNum : undefined,
      fontStyle: fontStyle ? fontStyle : undefined,
      letterSpacing: letterSpacing || undefined,
    }
  );

  const cached = CachedTextToPathResults.get(cacheKey);

  if (cached && cached.type === 'err') {
    return cached.message;
  }
  return null;
};

export const textToPath = async (
  text: string,
  size: { width: number | null; height: number | null } | null,
  params: TextToPathParams
): Promise<{ type: 'ok'; verts: Float32Array; indices: Uint32Array } | { type: 'err'; message: string }> => {
  const tess = await PathTess.get();

  const res = await fetch('https://3d.ameo.design/text-to-path/generate', {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      ...DEFAULT_TEXT_TO_PATH_PARAMS,
      ...params,
      text,
    }),
  });

  if (!res.ok) {
    const bodyText = await res.text().catch(() => '<unable to read response body>');
    console.error('Text to path generation failed:', res.status, res.statusText, bodyText);
    const out = { type: 'err' as const, message: `Failed to generate path: ${bodyText}` };
    const cacheKey = buildCacheKey(text, size, params);
    CachedTextToPathResults.set(cacheKey, out);
    return out;
  }

  const data = await res.json();
  if ('error' in data) {
    const out = { type: 'err' as const, message: data.error };
    const cacheKey = buildCacheKey(text, size, params);
    CachedTextToPathResults.set(cacheKey, out);
    return out;
  } else if (!('path' in data)) {
    console.error('Invalid response data:', data);
    const out = { type: 'err' as const, message: 'Invalid response from server' };
    const cacheKey = buildCacheKey(text, size, params);
    CachedTextToPathResults.set(cacheKey, out);
    return out;
  }

  const path = data.path as string;
  const ptr = tess.tessellate_path(path, size?.width ?? 0, size?.height ?? 0);
  if (!ptr) {
    const err = tess.get_tessellate_path_error();
    const out = { type: 'err' as const, message: `Path tessellation failed: ${err}` };
    const cacheKey = buildCacheKey(text, size, params);
    CachedTextToPathResults.set(cacheKey, out);
    return out;
  }

  const verts = tess.take_tess_output_verts(ptr);
  const indices = tess.take_tess_output_indices(ptr);
  tess.free_tess_output(ptr);

  const out = { type: 'ok' as const, verts, indices };
  const cacheKey = buildCacheKey(text, size, params);
  CachedTextToPathResults.set(cacheKey, out);
  return out;
};
