import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { buildPylonsMaterials } from 'src/viz/parkour/regions/pylons/materials';
import { Score, type ScoreThresholds } from 'src/viz/parkour/TimeDisplay.svelte';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import { createSignboard, type CreateSignboardArgs } from 'src/viz/helpers/signboardBuilder';
import type { CustomShaderMaterial } from 'src/viz/shaders/customShader';
import { goto } from '$app/navigation';

const locations = {
  spawn: {
    pos: new THREE.Vector3(-0.03534030541777611, 5.8327313423156735, 0.07172049582004547),
    rot: new THREE.Vector3(-0.2642763267949012, -2.1768726535898075, 1.6245929912462045e-15),
  },
};

const setupScene = (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig,
  shinyPatchworkStoneMaterial: CustomShaderMaterial
) => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const sunPos = new THREE.Vector3(0, 80, 0);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
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
  sunLight.shadow.camera.far = 100;
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

  const addStartSignpost = (
    pos: THREE.Vector3,
    text: string,
    paramOverrides: Partial<CreateSignboardArgs> = {}
  ) => {
    const sign = createSignboard({
      width: 10,
      height: 5,
      canvasWidth: 500,
      canvasHeight: 250,
      fontSize: 20,
      text,
      align: 'top-left',
      ...paramOverrides,
    });
    sign.position.copy(pos);
    sign.rotation.y = -Math.PI / 2;
    sign.updateMatrixWorld();
    viz.scene.add(sign);
    return sign;
  };

  addStartSignpost(
    new THREE.Vector3(10, 7, 4),
    'Your goal is simple:\n\nGet to the finish as fast as you can'
  );
  addStartSignpost(new THREE.Vector3(10, 7, 18), 'Just a few tips to get you started...');
  addStartSignpost(
    new THREE.Vector3(10, 7, 32),
    'You move faster while in the air than while walking, so stay airborne as much as possible for maximum speed'
  );
  addStartSignpost(
    new THREE.Vector3(10, 7, 46),
    "The timer doesn't start until after you jump for the first time, so you can take your time lining up your start on the spawn platform"
  );

  addStartSignpost(
    new THREE.Vector3(22, 17, 110.7),
    "That transparent wall was a checkpoint.  You'll respawn here if you die or fall."
  );
  addStartSignpost(
    new THREE.Vector3(22, 17, 124.7),
    "Press 'F' to reset to the start of the level if you mess up or miss a jump"
  );

  const sign = addStartSignpost(
    new THREE.Vector3(16, 18, 137.3),
    'This is a dash token.\n\nPicking these up will give you a dash charge that you can use by pressing Left Shift\n\n(You can find all keybinds in the pause menu under "Controls" by pressing Escape)',
    { width: 10 * 0.6, height: 5 * 0.6, canvasWidth: 500 * 0.6, canvasHeight: 250 * 0.6, fontSize: 20 * 0.6 }
  );
  sign.rotation.y = Math.PI;

  addStartSignpost(
    new THREE.Vector3(22, 18, 137.3 + 12),
    'Dashing is directional, so look in the direction you want to go before dashing.\n\nYou can dash while jumping as well to gain extra height'
  );

  const sign1 = addStartSignpost(
    new THREE.Vector3(84, 25, 180),
    "Congrats on beating your first level!\n\nYou'll figure the rest out as you go.  Have fun!"
  );
  sign1.rotation.y = 0;

  const middle = new THREE.Vector3(103, 25, 190);
  const nexusTeleporterPos = new THREE.Vector3(112, 25, 182);
  const sign2 = addStartSignpost(
    nexusTeleporterPos.clone().add(new THREE.Vector3(2, 5, -2)),
    'Back to Nexus',
    {
      align: 'center',
      fontSize: 40,
      width: 10 * 0.5,
      height: 5 * 0.5,
      canvasWidth: 500 * 0.5,
      canvasHeight: 250 * 0.5,
    }
  );
  sign2.lookAt(middle);

  const nextLevelTeleporterPos = new THREE.Vector3(112, 25, 202);
  const sign3 = addStartSignpost(
    nextLevelTeleporterPos.clone().add(new THREE.Vector3(2, 5, 2)),
    'Play Next Level',
    {
      align: 'center',
      fontSize: 40,
      width: 10 * 0.5,
      height: 5 * 0.5,
      canvasWidth: 500 * 0.5,
      canvasHeight: 250 * 0.5,
    }
  );
  sign3.lookAt(middle);

  const bobbers: THREE.Mesh[] = [];
  const spinners: THREE.Mesh[] = [];
  let backToNexusTP!: THREE.Mesh;
  let nextLevelTP!: THREE.Mesh;
  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.startsWith('spin') || obj.name.startsWith('backtonexus')) {
      bobbers.push(obj);
      if (obj.name.startsWith('spin') && !obj.name.includes('2')) {
        obj.material = shinyPatchworkStoneMaterial;
      }
    }

    if (obj.name.startsWith('spin')) {
      spinners.push(obj);
    }

    if (obj.name.startsWith('backtonexus')) {
      backToNexusTP = obj;
    }
    if (obj.name.startsWith('spin2')) {
      nextLevelTP = obj;
    }
  });

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: nextLevelTP }, () => {
      goto('/nexus');
    });

    fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: backToNexusTP }, () => {
      goto('/movement_v2');
    });
  });

  const originalBobberYs = bobbers.map(bobber => bobber.position.y);
  viz.registerBeforeRenderCb(curTimeSeconds => {
    for (let spinnerIx = 0; spinnerIx < spinners.length; spinnerIx += 1) {
      const spinner = spinners[spinnerIx];
      const spinSpeedRadsPerSecond = 0.5 + spinnerIx * 0.5;
      const spinOffsetRads = spinnerIx * Math.PI * 0.5;
      const spinSpeed = (curTimeSeconds + spinOffsetRads) * spinSpeedRadsPerSecond;
      spinner.rotation.y = spinSpeed;
    }

    const bobsPerSecond = 0.44;
    const bobHeight = 1.1;
    for (let bobberIx = 0; bobberIx < bobbers.length; bobberIx += 1) {
      bobbers[bobberIx].position.y =
        originalBobberYs[bobberIx] + Math.sin(curTimeSeconds * bobsPerSecond * 2 * Math.PI) * bobHeight;
    }
  });
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const { checkpointMat, greenMosaic2Material, goldMaterial, shinyPatchworkStoneMaterial } =
    await buildPylonsMaterials(viz, loadedWorld);

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: Infinity,
    [Score.S]: Infinity,
    [Score.A]: Infinity,
    [Score.B]: Infinity,
  };

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
    'tutorial',
    true
  );

  setupScene(viz, loadedWorld, vizConf, shinyPatchworkStoneMaterial);

  initPylonsPostprocessing(viz, vizConf, true);

  return pkManager.buildSceneConfig();
};
