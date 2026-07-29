// Loader for baked-model binaries (utah_teapot / stanford_bunny), fetched lazily via the
// `model_data` async dep instead of shipping the vertex data inside the geoscript wasm blob.
// Format (see scripts/build-model-bins.mjs): [u32 n_verts][u32 n_indices][f32*3*n_verts][u16*n_indices].

// URLs are configured by the caller via `setModelDataURLs` rather than imported with `?url` here,
// so this module can be included in a `?worker` graph without Vite emitting duplicate assets.
let URLsByName: Record<string, string> = {};
export const setModelDataURLs = (urls: Record<string, string>) => {
  URLsByName = urls;
};

const Loaded = new Map<string, { verts: Float32Array; indices: Uint32Array }>();
const Pending = new Map<string, Promise<void>>();

const loadOne = (name: string): Promise<void> => {
  if (Loaded.has(name)) {
    return Promise.resolve();
  }
  let p = Pending.get(name);
  if (!p) {
    const url = URLsByName[name];
    if (!url) {
      return Promise.reject(new Error(`no URL registered for baked model ${name}`));
    }
    p = fetch(url)
      .then(r => r.arrayBuffer())
      .then(buf => {
        const [nVerts, nIndices] = new Uint32Array(buf, 0, 2);
        Loaded.set(name, {
          verts: new Float32Array(buf, 8, nVerts * 3),
          indices: Uint32Array.from(new Uint16Array(buf, 8 + nVerts * 12, nIndices)),
        });
        Pending.delete(name);
      });
    Pending.set(name, p);
  }
  return p;
};

/** Loads the named models, or every registered model if `names` is empty/omitted (preload path). */
export const initModelData = (names?: string[]): Promise<void> =>
  Promise.all((names?.length ? names : Object.keys(URLsByName)).map(loadOne)).then(() => {});

export const model_data_is_loaded = (name: string): boolean => Loaded.has(name);
export const model_data_get_verts = (name: string): Float32Array => Loaded.get(name)!.verts;
export const model_data_get_indices = (name: string): Uint32Array => Loaded.get(name)!.indices;
