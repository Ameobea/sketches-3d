import { ScenesByName } from 'src/viz/scenes';
import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import { getScenePreloadUrls } from 'src/viz/levelDef/preloadUrls';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params }) => {
  const sceneName = params.scene;
  const sceneDef = ScenesByName[sceneName];

  if (sceneDef?.useSceneDef) {
    const levelDef = await loadLevelData(sceneName);
    return { sceneName, levelDef, preloadUrls: getScenePreloadUrls(levelDef) };
  }

  return { sceneName, levelDef: null, preloadUrls: getScenePreloadUrls(null) };
};
