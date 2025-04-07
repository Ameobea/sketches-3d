import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { buildPylonsMaterials } from 'src/viz/parkour/regions/pylons/materials';
import { Score, type ScoreThresholds } from 'src/viz/parkour/TimeDisplay.svelte';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { loadNamedTextures } from 'src/viz/textureLoading';
import RidgesColorShader from './shaders/ridges/color.frag?raw';
import RidgesRoughnessShader from './shaders/ridges/roughness.frag?raw';

const locations = {
  spawn: {
    pos: new THREE.Vector3(-4, 14.56807, 5.98513),
    rot: new THREE.Vector3(0, Math.PI / 2, 0),
  },
  1: {
    pos: new THREE.Vector3(-68.26718139648438, 16.945634841918945, 54.989990234375),
    rot: new THREE.Vector3(-0.18599999999999045, -1.3062142875642406, 1.4827423374346092e-15),
  },
};

const setupScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig) => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 1.3);
  viz.scene.add(ambientLight);

  const sunPos = new THREE.Vector3(20, 50, -20);
  const sunLight = new THREE.DirectionalLight(0xffffff, 4.6);
  const shadowMapSize = {
    [GraphicsQuality.Low]: 1024,
    [GraphicsQuality.Medium]: 2048,
    [GraphicsQuality.High]: 4096,
  }[vizConf.graphics.quality];
  sunLight.castShadow = true;
  // sunLight.shadow.bias = 0.01;
  sunLight.shadow.mapSize.width = shadowMapSize;
  sunLight.shadow.mapSize.height = shadowMapSize;
  sunLight.shadow.camera.near = 0.1;
  sunLight.shadow.camera.far = 200;
  sunLight.shadow.camera.left = -250;
  sunLight.shadow.camera.right = 250;
  sunLight.shadow.camera.top = 250;
  sunLight.shadow.camera.bottom = -250;
  sunLight.shadow.camera.updateProjectionMatrix();
  sunLight.matrixWorldNeedsUpdate = true;
  sunLight.updateMatrixWorld();
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  // const shadowCameraHelper = new THREE.CameraHelper(sunLight.shadow.camera);
  // viz.scene.add(shadowCameraHelper);

  const metalMat = buildCustomShader({ color: 0xdddddd, metalness: 0.8, roughness: 0.2 });

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.startsWith('metal')) {
      console.log(obj);
      obj.material = metalMat;
    }
  });
};

const loadLevelMats = async () => {
  const loader = new THREE.ImageBitmapLoader();
  return loadNamedTextures(loader, { buildingTexture: 'https://i.ameo.link/bdu.jpg' });
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const [
    { checkpointMat, greenMosaic2Material, goldMaterial, shinyPatchworkStoneMaterial, pylonMaterial },
    { buildingTexture },
  ] = await Promise.all([buildPylonsMaterials(viz, loadedWorld), loadLevelMats()]);

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: 30.5,
    [Score.S]: 30.75,
    [Score.A]: 34,
    [Score.B]: 38,
  };

  const slatsMat = buildCustomShader(
    {
      map: buildingTexture,
      metalness: 0.99,
      uvTransform: new THREE.Matrix3().scale(1.8900348, 1.8900348),
    },
    {
      roughnessShader: `
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(smoothstep(0.22, 0.6, ctx.diffuseColor.r), 2.5) * 1.4;
  shinyness = clamp(shinyness, 0.0, 0.8);
  return 1. - shinyness;
}`,
    }
  );

  const ridgesMat = buildCustomShader(
    {
      metalness: 0.89,
      iridescence: 0.2,
    },
    {
      colorShader: RidgesColorShader,
      roughnessShader: RidgesRoughnessShader,
    }
  );

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.startsWith('roofslats')) {
      obj.material = slatsMat;
    }
    if (obj.name.startsWith('ridges')) {
      obj.material = ridgesMat;
    }
    if (obj.name.startsWith('ledge')) {
      obj.material = shinyPatchworkStoneMaterial;
    }
  });

  const pkManager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    locations,
    scoreThresholds,
    {
      dashToken: { core: greenMosaic2Material, ring: goldMaterial },
      checkpoint: checkpointMat,
    },
    'cornered',
    true
  );

  setupScene(viz, loadedWorld, vizConf);

  initPylonsPostprocessing(viz, vizConf, true);

  return pkManager.buildSceneConfig();
};
