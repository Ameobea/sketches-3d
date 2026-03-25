import { loadLevelData } from 'src/viz/levelDef/loadLevelData.server';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => ({
  levelDef: loadLevelData('scene_def_test'),
});
