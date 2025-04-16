import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { buildPylonsMaterials } from 'src/viz/parkour/regions/pylons/materials';
import { Score, type ScoreThresholds } from 'src/viz/parkour/TimeDisplay.svelte';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import { generateNormalMapFromTexture, loadTexture } from 'src/viz/textureLoading';
import { buildCustomShader } from 'src/viz/shaders/customShader';

const locations = {
  spawn: {
    pos: new THREE.Vector3(94, 1, -94),
    rot: new THREE.Vector3(0, (1.5 * Math.PI) / 2, 0),
  },
  1: {
    pos: new THREE.Vector3(-91.3979721069336, 3.276488065719604, 96.122314453125),
    rot: new THREE.Vector3(-0.038596326794894095, -0.663205509807639, -2.3609822588590132e-15),
  },
};

const makeRotatingPlatforms = (
  viz: Viz,
  pkMgr: ParkourManager,
  center: THREE.Vector3,
  radius: number,
  speed: number,
  dir: 'cw' | 'ccw',
  count: number,
  baseMesh: THREE.Mesh
) => {
  // platforms rotate around the center point in a square, repeating their motion infinitely
  const circumference = radius * 2 * 4;
  const secondsPerLap = circumference / speed;

  const topLeft = new THREE.Vector3(center.x - radius, center.y, center.z + radius);
  const topRight = new THREE.Vector3(center.x + radius, center.y, center.z + radius);
  const bottomLeft = new THREE.Vector3(center.x - radius, center.y, center.z - radius);
  const bottomRight = new THREE.Vector3(center.x + radius, center.y, center.z - radius);
  const getPos = (startPhase: number, secondsSinceStart: number) => {
    const rawPhase = ((secondsSinceStart + startPhase * secondsPerLap) / secondsPerLap) % 1;
    const phase = dir === 'ccw' ? 1 - rawPhase : rawPhase;

    if (phase < 0.25) {
      return topLeft.clone().lerp(topRight, phase * 4);
    } else if (phase < 0.5) {
      return topRight.clone().lerp(bottomRight, (phase - 0.25) * 4);
    } else if (phase < 0.75) {
      return bottomRight.clone().lerp(bottomLeft, (phase - 0.5) * 4);
    } else {
      return bottomLeft.clone().lerp(topLeft, (phase - 0.75) * 4);
    }
  };

  for (let i = 0; i < count; i += 1) {
    const startPhase = i / count;
    const mesh = baseMesh.clone();
    mesh.position.copy(getPos(startPhase, 0));
    viz.scene.add(mesh);
    pkMgr.makeSlider(mesh, {
      getPos: (curTimeSeconds: number, secondsSinceSpawn: number) => getPos(startPhase, secondsSinceSpawn),
      removeOnReset: false,
    });
  }
};

const setupScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig) => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 1.58);
  viz.scene.add(ambientLight);

  const sunPos = new THREE.Vector3(20, 50, -20);
  const sunLight = new THREE.DirectionalLight(0xffffff, 4.3);
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

  const spotlight = new THREE.SpotLight(0xf2dd99, 5.5);
  spotlight.position.set(-75, 100, 75);
  spotlight.angle = Math.PI / 8;
  spotlight.penumbra = 0.5;
  spotlight.decay = 0;
  spotlight.distance = 0;
  spotlight.castShadow = false;
  spotlight.target.position.set(-100, 20, 60);
  spotlight.target.updateMatrixWorld();
  viz.scene.add(spotlight);
  viz.scene.add(spotlight.target);

  const helper = new THREE.SpotLightHelper(spotlight);
  viz.scene.add(helper);
};

const loadCustomMats = async (loader: THREE.ImageBitmapLoader) => {
  const towerCeilingTextureP = loadTexture(
    loader,
    'https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000008.1761839491.png'
  );
  const towerCeilingCombinedDiffuseNormalTextureP = towerCeilingTextureP.then(towerCeilingTexture =>
    generateNormalMapFromTexture(towerCeilingTexture, {}, true)
  );
  const towerTrimTextureP = loadTexture(
    loader,
    'https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000008.1999177113.png'
  );
  const towerTrimTextureCombinedDiffuseNormalTextureP = towerTrimTextureP.then(towerTrimTexture =>
    generateNormalMapFromTexture(towerTrimTexture, {}, true)
  );

  const towerPlinthArchTextureP = loadTexture(loader, 'https://i.ameo.link/bip.jpg');
  const towerPlinthArchTextureCombinedDiffuseNormalTextureP = towerPlinthArchTextureP.then(
    towerPlinthArchTexture => generateNormalMapFromTexture(towerPlinthArchTexture, {}, true)
  );

  const [
    towerCeilingCombinedDiffuseNormalTexture,
    towerTrimCombinedDiffuseNormalTexture,
    towerPlinthArchTexture,
  ] = await Promise.all([
    towerCeilingCombinedDiffuseNormalTextureP,
    towerTrimTextureCombinedDiffuseNormalTextureP,
    towerPlinthArchTextureCombinedDiffuseNormalTextureP,
  ]);

  const towerMat = buildCustomShader(
    {
      color: new THREE.Color(0x8b8b8c),
      metalness: 0.001,
      roughness: 0.97,
      map: towerCeilingCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(20.14, 20.14),
      mapDisableDistance: null,
      normalScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );

  const towerTrimMat = buildCustomShader(
    {
      color: new THREE.Color(0xa7a6a5),
      metalness: 0,
      roughness: 1,
      map: towerTrimCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(22.14, 22.14),
      mapDisableDistance: null,
      normalScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );

  const plinthArchMat = buildCustomShader(
    {
      color: new THREE.Color(0xcccccc2),
      metalness: 0.001,
      roughness: 0.77,
      map: towerPlinthArchTexture,
      uvTransform: new THREE.Matrix3().scale(0.05 * 2, 0.1 * 2),
      mapDisableDistance: null,
      normalScale: 4,
      ambientLightScale: 1,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      useGeneratedUVs: true,
      // useTriplanarMapping: true,
      // tileBreaking: { type: 'neyret', patchScale: 2 },
    }
  );

  return { towerMat, towerTrimMat, plinthArchMat };
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const loader = new THREE.ImageBitmapLoader();
  const [
    { checkpointMat, greenMosaic2Material, goldMaterial, shinyPatchworkStoneMaterial, pylonMaterial },
    { towerMat, towerTrimMat, plinthArchMat },
  ] = await Promise.all([buildPylonsMaterials(viz, loadedWorld, loader), loadCustomMats(loader)]);

  // TODO
  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: 30.5,
    [Score.S]: 30.75,
    [Score.A]: 34,
    [Score.B]: 38,
  };

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.startsWith('tower')) {
      obj.material = towerMat;
    } else if (obj.name.startsWith('trim')) {
      obj.material = towerTrimMat;
    } else if (obj.name.startsWith('ridge')) {
      obj.material = shinyPatchworkStoneMaterial;
    } else if (obj.name.startsWith('jump_plat')) {
      obj.material = plinthArchMat;
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
    'stronghold',
    true
  );

  viz.collisionWorldLoadedCbs.push(() => {
    const placeholder = loadedWorld.getObjectByName('placeholder') as THREE.Mesh | undefined;
    if (placeholder) {
      placeholder.removeFromParent();
      viz.fpCtx!.removeCollisionObject(placeholder.userData.rigidBody);
    }

    const plat1 = loadedWorld.getObjectByName('plat1') as THREE.Mesh;
    viz.fpCtx!.removeCollisionObject(plat1.userData.rigidBody, 'plat1');
    delete plat1.userData.rigidBody;

    const platSpeed = 2.8;
    makeRotatingPlatforms(viz, pkManager, new THREE.Vector3(0, -5, 0), 82, platSpeed, 'cw', 45, plat1);
    makeRotatingPlatforms(
      viz,
      pkManager,
      new THREE.Vector3(0, -5, 0),
      82 * (40 / 45),
      platSpeed,
      'ccw',
      40,
      plat1
    );

    makeRotatingPlatforms(viz, pkManager, new THREE.Vector3(0, 55, 0), 42.5, platSpeed, 'ccw', 33, plat1);
    makeRotatingPlatforms(
      viz,
      pkManager,
      new THREE.Vector3(0, 55, 0),
      42.5 * (26 / 33),
      platSpeed,
      'cw',
      26,
      plat1
    );
    plat1.removeFromParent();
  });

  setupScene(viz, loadedWorld, vizConf);

  initPylonsPostprocessing(viz, vizConf, true);

  return pkManager.buildSceneConfig();
};
