import { ScenesByName } from 'src/viz/scenes';
import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params }) => {
  const sceneName = params.scene;
  const sceneDef = ScenesByName[sceneName];

  if (sceneDef?.useSceneDef) {
    return { sceneName, levelDef: await loadLevelData(sceneName) };
  }

  return { sceneName, levelDef: null };
};
