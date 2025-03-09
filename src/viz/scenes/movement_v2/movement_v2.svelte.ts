import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { Score, type ScoreThresholds } from '../../parkour/TimeDisplay.svelte';
import { buildMaterials } from '../../parkour/regions/pylons/materials';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import { ParkourManager } from '../../parkour/ParkourManager.svelte';

const locations = {
  spawn: {
    pos: new THREE.Vector3(2.82073, 3.56807, 5.98513),
    rot: new THREE.Vector3(0, Math.PI, 0),
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
    [Score.SPlus]: 25.5,
    [Score.S]: 27,
    [Score.A]: 30,
    [Score.B]: 38,
  };

  const manager = new ParkourManager(viz, loadedWorld, vizConf, locations, scoreThresholds, {
    dashToken: { core: greenMosaic2Material, ring: goldMaterial },
    checkpoint: checkpointMat,
  });

  initPylonsPostprocessing(viz, vizConf);

  return manager.buildSceneConfig();
};
