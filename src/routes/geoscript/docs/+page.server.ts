import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import type { PageServerLoad } from './$types';
import type { BuiltinFnDefs, PopulatedFnExample, UnpopulatedBuiltinFnDefs } from './types';
import { getComposition, getCompositionLatest } from 'src/geoscript/geotoyAPIClient';

const Geoscript = new AsyncOnce((fetch: typeof window.fetch) =>
  import('src/viz/wasmComp/geoscript_repl').then(async engine => {
    await engine.default(fetch('/geoscript_repl_bg.wasm'));
    return engine;
  })
);

export const load: PageServerLoad = async ({ fetch }): Promise<{ builtinFnDefs: BuiltinFnDefs }> => {
  const geoscript = await Geoscript.get(fetch);

  const unpopulatedDefs = JSON.parse(
    geoscript.geoscript_repl_get_serialized_builtin_fn_defs()
  ) as UnpopulatedBuiltinFnDefs;

  const allReferencedCompositionIDsSet = new Set<number>();
  for (const fnDef of Object.values(unpopulatedDefs)) {
    for (const example of fnDef.examples) {
      allReferencedCompositionIDsSet.add(example.composition_id);
    }
  }

  const allReferencedCompositionIDs = Array.from(allReferencedCompositionIDsSet);
  const compositionIndicesByID = new Map<number, number>();
  for (let i = 0; i < allReferencedCompositionIDs.length; i++) {
    compositionIndicesByID.set(allReferencedCompositionIDs[i], i);
  }

  const baseURL = 'https://3d.ameo.design/geotoy_api';
  const compositionVersionsP = Promise.all(
    allReferencedCompositionIDs.map(id => getCompositionLatest(id, fetch, undefined, undefined, baseURL))
  );
  const compositionsP = Promise.all(
    allReferencedCompositionIDs.map(id => getComposition(id, fetch, undefined, undefined, baseURL))
  );
  const [compositions, versions] = await Promise.all([compositionsP, compositionVersionsP]);

  const defs = Object.fromEntries(
    Object.entries(unpopulatedDefs).map(([name, unpopulatedDef]) => {
      const populatedExamples = unpopulatedDef.examples.map((example): PopulatedFnExample => {
        const compositionIndex = compositionIndicesByID.get(example.composition_id)!;
        const composition = compositions[compositionIndex];
        const version = versions[compositionIndex];
        return {
          composition_id: example.composition_id,
          composition,
          version,
        };
      });

      return [name, { ...unpopulatedDef, examples: populatedExamples }];
    })
  );

  return { builtinFnDefs: defs };
};
