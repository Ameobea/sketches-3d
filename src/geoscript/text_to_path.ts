// Cache: raw SVG path string (no width/height in cache key — scaling is now Rust-side)
const CachedTextToSvg = new Map<string, { type: 'ok'; svgPath: string } | { type: 'err'; message: string }>();

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

const buildCacheKey = (text: string, params: TextToPathParams): string =>
  [
    text,
    params.fontFamily,
    params.fontSize || 0,
    params.fontWeight || '',
    params.fontStyle || '',
    params.letterSpacing || 0,
  ]
    .map(x => `${x}`)
    .join('|');

export const get_cached_text_to_svg_path = (
  text: string,
  fontFamily: string,
  fontSize: number,
  fontWeight: string,
  fontStyle: string,
  letterSpacing: number
): string | null => {
  const fontWeightConverted = fontWeight
    ? isNaN(Number(fontWeight))
      ? fontWeight
      : Number(fontWeight)
    : undefined;
  const cacheKey = buildCacheKey(text, {
    fontFamily,
    fontSize: fontSize || undefined,
    fontWeight: fontWeightConverted,
    fontStyle: (fontStyle || undefined) as 'normal' | 'italic' | 'oblique' | undefined,
    letterSpacing: letterSpacing || undefined,
  });
  const cached = CachedTextToSvg.get(cacheKey);
  if (cached && cached.type === 'ok') {
    return cached.svgPath;
  }
  return null;
};

export const get_cached_text_to_svg_err = (
  text: string,
  fontFamily: string,
  fontSize: number,
  fontWeight: string,
  fontStyle: string,
  letterSpacing: number
): string | null => {
  const fontWeightConverted = fontWeight
    ? isNaN(Number(fontWeight))
      ? fontWeight
      : Number(fontWeight)
    : undefined;
  const cacheKey = buildCacheKey(text, {
    fontFamily,
    fontSize: fontSize || undefined,
    fontWeight: fontWeightConverted,
    fontStyle: (fontStyle || undefined) as 'normal' | 'italic' | 'oblique' | undefined,
    letterSpacing: letterSpacing || undefined,
  });
  const cached = CachedTextToSvg.get(cacheKey);
  if (cached && cached.type === 'err') {
    return cached.message;
  }
  return null;
};

export const textToSvg = async (text: string, params: TextToPathParams): Promise<void> => {
  const mergedParams = { ...DEFAULT_TEXT_TO_PATH_PARAMS, ...params };
  const cacheKey = buildCacheKey(text, mergedParams);

  if (CachedTextToSvg.has(cacheKey)) {
    return;
  }

  const res = await fetch('https://3d.ameo.design/text-to-path/generate', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ ...mergedParams, text }),
  });

  if (!res.ok) {
    const bodyText = await res.text().catch(() => '<unable to read response body>');
    console.error('Text to SVG generation failed:', res.status, res.statusText, bodyText);
    CachedTextToSvg.set(cacheKey, {
      type: 'err',
      message: `Failed to generate path: ${bodyText}`,
    });
    return;
  }

  const data = await res.json();
  if ('error' in data) {
    CachedTextToSvg.set(cacheKey, { type: 'err', message: data.error });
  } else if (!('path' in data)) {
    console.error('Invalid response data:', data);
    CachedTextToSvg.set(cacheKey, { type: 'err', message: 'Invalid response from server' });
  } else {
    CachedTextToSvg.set(cacheKey, { type: 'ok', svgPath: data.path as string });
  }
};
