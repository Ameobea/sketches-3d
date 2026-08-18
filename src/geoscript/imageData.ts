// Host-side decode for the `load_image` geoscript builtin: data URIs are decoded with
// `createImageBitmap` (no color management, un-premultiplied) and cached by URI, reached
// from wasm via the `image_data` async dep.

const Loaded = new Map<string, { width: number; height: number; rgba: Uint8ClampedArray }>();
const Pending = new Map<string, Promise<void>>();

const loadOne = (uri: string): Promise<void> => {
  if (Loaded.has(uri)) {
    return Promise.resolve();
  }
  let p = Pending.get(uri);
  if (!p) {
    p = fetch(uri)
      .then(r => r.blob())
      .then(blob =>
        // Chrome rejects some PNGs (e.g. grayscale) when colorSpaceConversion is 'none'
        createImageBitmap(blob, { premultiplyAlpha: 'none', colorSpaceConversion: 'none' }).catch(() =>
          createImageBitmap(blob)
        )
      )
      .then(bmp => {
        const canvas = new OffscreenCanvas(bmp.width, bmp.height);
        const ctx = canvas.getContext('2d')!;
        ctx.drawImage(bmp, 0, 0);
        const img = ctx.getImageData(0, 0, bmp.width, bmp.height);
        Loaded.set(uri, { width: bmp.width, height: bmp.height, rgba: img.data });
        bmp.close();
      })
      .finally(() => {
        Pending.delete(uri);
      });
    Pending.set(uri, p);
  }
  return p;
};

export const initImageData = (uris?: string[]): Promise<void> =>
  Promise.all((uris ?? []).map(loadOne)).then(() => {});

export const image_data_is_loaded = (uri: string): boolean => Loaded.has(uri);
export const image_data_get_dims = (uri: string): Uint32Array => {
  const e = Loaded.get(uri)!;
  return new Uint32Array([e.width, e.height]);
};
export const image_data_get_rgba = (uri: string): Uint8Array =>
  new Uint8Array(Loaded.get(uri)!.rgba.buffer.slice(0));
