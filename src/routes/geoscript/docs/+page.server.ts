import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import type { PageServerLoad } from './$types';
import type { BuiltinFnDefs } from './types';

const Geoscript = new AsyncOnce((fetch: typeof window.fetch) =>
  import('src/viz/wasmComp/geoscript_repl').then(async engine => {
    await engine.default(fetch('/geoscript_repl_bg.wasm'));
    return engine;
  })
);

let cachedDefs: BuiltinFnDefs | null = null;

export const load: PageServerLoad = async ({ fetch }): Promise<{ builtinFnDefs: BuiltinFnDefs }> => {
  if (!cachedDefs) {
    const geoscript = await Geoscript.get(fetch);

    cachedDefs = JSON.parse(geoscript.geoscript_repl_get_serialized_builtin_fn_defs()) as BuiltinFnDefs;
  }

  return { builtinFnDefs: cachedDefs };
};
