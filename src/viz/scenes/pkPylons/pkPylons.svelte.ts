import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { Score, type ScoreThresholds } from '../../parkour/TimeDisplay.svelte';
import { buildMaterials } from '../../parkour/regions/pylons/materials';
import { initPylonsPostprocessing } from './postprocessing';
import { ParkourManager } from '../../parkour/ParkourManager.svelte';

const locations = {
  spawn: {
    pos: new THREE.Vector3(4.5, 2, 6),
    rot: new THREE.Vector3(-0.1, 1.378, 0),
  },
  '3': {
    pos: new THREE.Vector3(-73.322, 27.647, -33.4451),
    rot: new THREE.Vector3(-0.212, -8.5, 0),
  },
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const { checkpointMat, greenMosaic2Material, goldMaterial } = await buildMaterials(viz, loadedWorld);

  const sunPos = new THREE.Vector3(200, 290, -135);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: 32.1,
    [Score.S]: 33.5,
    [Score.A]: 40,
    [Score.B]: 50,
  };

  const manager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    locations,
    scoreThresholds,
    {
      dashToken: { core: greenMosaic2Material, ring: goldMaterial },
      checkpoint: checkpointMat,
    },
    'pk_pylons',
    false
  );

  initPylonsPostprocessing(viz, vizConf);

  return manager.buildSceneConfig();
};
