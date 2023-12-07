import { get, writable, type Writable } from 'svelte/store';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { generateNormalMapFromTexture, loadNamedTextures, loadTexture } from 'src/viz/textureLoading';
import type { SceneConfig } from '..';
import BridgeMistColorShader from '../../shaders/bridge2/bridge_top_mist/color.frag?raw';
import { CollectablesCtx, initCollectables } from './collectables';
import { DashToken } from './DashToken';
import TimerDisplay from './TimerDisplay.svelte';
import TimeDisplay, { Score, type ScoreThresholds } from './TimeDisplay.svelte';

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

const buildMaterials = async (viz: VizState) => {
  const loader = new THREE.ImageBitmapLoader();
  const towerPlinthPedestalTextureP = loadTexture(
    loader,
    'https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000005.1476533049.png'
  );
  const towerPlinthPedestalTextureCombinedDiffuseNormalTextureP = towerPlinthPedestalTextureP.then(
    towerPlinthPedestalTexture => generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
  );

  const bgTextureP = (async () => {
    const bgImage = await loader.loadAsync('https://i.ameo.link/bqn.jpg');
    const bgTexture = new THREE.Texture(bgImage);
    bgTexture.mapping = THREE.EquirectangularRefractionMapping;
    bgTexture.needsUpdate = true;
    return bgTexture;
  })();

  const [
    bgTexture,
    towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
    {
      shinyPatchworkStoneAlbedo,
      shinyPatchworkStoneNormal,
      shinyPatchworkStoneRoughness,
      greenMosaic2Albedo,
      greenMosaic2Normal,
      greenMosaic2Roughness,
      goldTextureAlbedo,
      goldTextureNormal,
      goldTextureRoughness,
    },
  ] = await Promise.all([
    bgTextureP,
    towerPlinthPedestalTextureCombinedDiffuseNormalTextureP,
    loadNamedTextures(loader, {
      shinyPatchworkStoneAlbedo: 'https://i.ameo.link/bqk.jpg',
      shinyPatchworkStoneNormal: 'https://i.ameo.link/bqm.jpg',
      shinyPatchworkStoneRoughness: 'https://i.ameo.link/bql.jpg',
      greenMosaic2Albedo: 'https://i.ameo.link/bqh.jpg',
      greenMosaic2Normal: 'https://i.ameo.link/bqi.jpg',
      greenMosaic2Roughness: 'https://i.ameo.link/bqj.jpg',
      goldTextureAlbedo: 'https://i.ameo.link/be0.jpg',
      goldTextureNormal: 'https://i.ameo.link/be2.jpg',
      goldTextureRoughness: 'https://i.ameo.link/bdz.jpg',
    }),
  ]);

  const pylonMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x898989),
      metalness: 0.18,
      roughness: 0.82,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 5.2,
    },
    {},
    {
      usePackedDiffuseNormalGBA: true,
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 0.9 },
    }
  );

  const checkpointMat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true },
    { colorShader: BridgeMistColorShader },
    { disableToneMapping: true }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => checkpointMat.setCurTimeSeconds(curTimeSeconds));

  const shinyPatchworkStoneMaterial = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      metalness: 0.5,
      roughness: 0.5,
      map: shinyPatchworkStoneAlbedo,
      normalMap: shinyPatchworkStoneNormal,
      roughnessMap: shinyPatchworkStoneRoughness,
      uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
      mapDisableDistance: null,
      normalScale: 1.2,
      ambientLightScale: 2,
    },
    {},
    {
      useGeneratedUVs: true,
      randomizeUVOffset: true,
      tileBreaking: { type: 'neyret', patchScale: 0.9 },
    }
  );

  const greenMosaic2Material = buildCustomShader(
    {
      color: new THREE.Color(0xffffff),
      metalness: 0.5,
      roughness: 0.5,
      map: greenMosaic2Albedo,
      normalMap: greenMosaic2Normal,
      roughnessMap: greenMosaic2Roughness,
      uvTransform: new THREE.Matrix3().scale(7.8, 7.8),
      mapDisableDistance: null,
      normalScale: 2.2,
      ambientLightScale: 2,
    },
    {},
    {
      // useTriplanarMapping: true
      // useGeneratedUVs: true,
    }
  );

  const goldMaterial = buildCustomShader(
    {
      map: goldTextureAlbedo,
      roughnessMap: goldTextureRoughness,
      normalMap: goldTextureNormal,
      color: new THREE.Color(0xaaaaaa),
      uvTransform: new THREE.Matrix3().scale(0.6, 0.6),
      normalScale: 4,
      roughness: 0.2,
    },
    {},
    {
      useTriplanarMapping: true,
    }
  );

  return {
    pylonMaterial,
    checkpointMat,
    bgTexture,
    shinyPatchworkStoneMaterial,
    greenMosaic2Material,
    goldMaterial,
  };
};

const initCheckpoints = (
  viz: VizState,
  loadedWorld: THREE.Group<THREE.Object3DEventMap>,
  checkpointMat: THREE.Material,
  dashTokensCtx: CollectablesCtx,
  curDashCharges: Writable<number>,
  onComplete: () => void
) => {
  let latestReachedCheckpointIx: number | null = 0;
  let dashChargesAtLastCheckpoint = 0;
  const setSpawnPoint = (pos: THREE.Vector3, rot: THREE.Vector3) => viz.fpCtx!.setSpawnPos(pos, rot);

  const parseCheckpointIx = (name: string) => {
    // names are like "checkpoint", "checkpoint001", "checkpoint002", etc.
    // "checkpoint" = 0
    const match = name.match(/checkpoint(\d+)/);
    if (!match) {
      return 0;
    }

    return parseInt(match[1], 10);
  };

  const ctx = initCollectables({
    viz,
    loadedWorld,
    collectableName: 'checkpoint',
    onCollect: checkpoint => {
      setSpawnPoint(
        checkpoint.position,
        new THREE.Vector3(viz.camera.rotation.x, viz.camera.rotation.y, viz.camera.rotation.z)
      );

      // TODO: sfx

      const checkpointIx = parseCheckpointIx(checkpoint.name);
      latestReachedCheckpointIx = checkpointIx;
      dashChargesAtLastCheckpoint = get(curDashCharges);
      if (checkpointIx === 1) {
        onComplete();
      }
    },
    material: checkpointMat,
    collisionRegionScale: new THREE.Vector3(1, 30, 1),
  });

  viz.collisionWorldLoadedCbs.push(fpCtx =>
    fpCtx.registerOnRespawnCb(() => {
      curDashCharges.set(dashChargesAtLastCheckpoint);

      const needle = `ck${latestReachedCheckpointIx === null ? 0 : latestReachedCheckpointIx + 1}`;
      const toRestore: THREE.Object3D[] = [];
      console.log(dashTokensCtx.hiddenCollectables);
      for (const obj of dashTokensCtx.hiddenCollectables) {
        if (obj.name.includes(needle)) {
          toRestore.push(obj);
        }
      }

      dashTokensCtx.restore(toRestore);
    })
  );

  const reset = () => {
    ctx.reset();
    latestReachedCheckpointIx = null;
    dashChargesAtLastCheckpoint = 0;
  };
  return reset;
};

const initDashTokens = (
  viz: VizState,
  loadedWorld: THREE.Group,
  coreMaterial: THREE.Material,
  ringMaterial: THREE.Material
) => {
  const dashCharges = writable(0);
  const base = loadedWorld.getObjectByName('dash_token')!;
  base.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.includes('ring')) {
      obj.material = ringMaterial;
    } else if (obj.name.includes('core')) {
      obj.material = coreMaterial;
    }
  });
  const dashTokenBase = new DashToken(viz, base);
  const ctx = initCollectables({
    viz,
    loadedWorld,
    collectableName: 'dash_token_loc',
    replacementObject: dashTokenBase,
    onCollect: () => {
      dashCharges.update(n => n + 1);
      // TODO: sfx
    },
    type: 'aabb',
  });
  return {
    dashCharges,
    ctx,
    reset: () => {
      ctx.reset();
      dashCharges.set(0);
    },
  };
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.8);
  viz.scene.add(ambientLight);

  const {
    pylonMaterial,
    checkpointMat,
    bgTexture,
    shinyPatchworkStoneMaterial,
    greenMosaic2Material,
    goldMaterial,
  } = await buildMaterials(viz);

  viz.scene.background = bgTexture;

  const sunPos = new THREE.Vector3(200, 290, -135);
  const sunLight = new THREE.DirectionalLight(0xffffff, 3.6);
  sunLight.position.copy(sunPos);
  viz.scene.add(sunLight);

  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.includes('sparkle')) {
      obj.material = shinyPatchworkStoneMaterial;
    } else {
      obj.material = pylonMaterial;
    }
  });

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: 32.1,
    [Score.S]: 33.5,
    [Score.A]: 40,
    [Score.B]: 50,
  };
  let curRunStartTimeSeconds: number | null = null;
  let winState: { winTimeSeconds: number; displayComp: TimeDisplay } | null = null;

  viz.collisionWorldLoadedCbs.push(fpCtx =>
    fpCtx.registerJumpCb(curTimeSeconds => {
      if (curRunStartTimeSeconds === null) {
        curRunStartTimeSeconds = curTimeSeconds;
      }
    })
  );

  const onWin = () => {
    const curTimeSeconds = viz.clock.getElapsedTime();

    const target = document.createElement('div');
    document.body.appendChild(target);
    const time = curTimeSeconds - (curRunStartTimeSeconds ?? 0);
    const displayComp = new TimeDisplay({ target, props: { scoreThresholds, time } });
    winState = { winTimeSeconds: curTimeSeconds, displayComp };

    viz.fpCtx!.setSpawnPos(locations.spawn.pos, locations.spawn.rot);
  };

  const {
    ctx: dashTokensCtx,
    dashCharges: curDashCharges,
    reset: resetDashes,
  } = initDashTokens(viz, loadedWorld, greenMosaic2Material, goldMaterial);
  const resetCheckpoints = initCheckpoints(
    viz,
    loadedWorld,
    checkpointMat,
    dashTokensCtx,
    curDashCharges,
    onWin
  );

  const target = document.createElement('div');
  document.body.appendChild(target);
  const timerDisplay = new TimerDisplay({ target, props: { curTime: 0 } });
  viz.registerAfterRenderCb(curTimeSeconds => {
    const elapsedSeconds = (() => {
      if (curRunStartTimeSeconds === null) {
        return 0;
      }

      if (winState) {
        return winState.winTimeSeconds - curRunStartTimeSeconds;
      }

      return curTimeSeconds - curRunStartTimeSeconds;
    })();
    timerDisplay.$$set({ curTime: elapsedSeconds });
  });

  const reset = () => {
    resetDashes();
    resetCheckpoints();
    viz.fpCtx!.teleportPlayer(locations.spawn.pos, locations.spawn.rot);
    viz.fpCtx!.reset();
    curRunStartTimeSeconds = null;
    winState?.displayComp.$destroy();
    winState = null;
    viz.fpCtx!.setSpawnPos(locations.spawn.pos, locations.spawn.rot);
  };

  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality, (composer, viz, quality) => {
    const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
      fogMinY: -60,
      fogMaxY: -4,
      fogColorLowDensity: new THREE.Vector3(0.42, 0.45, 0.48),
      fogColorHighDensity: new THREE.Vector3(0.8, 0.8, 0.8),
      ambientLightColor: new THREE.Color(0xffffff),
      ambientLightIntensity: 1.2,
      heightFogStartY: -30,
      heightFogEndY: -24,
      maxRayLength: 300,
      minStepLength: 0.1,
      noiseBias: 0.4,
      heightFogFactor: 0.14,
      noisePow: 1.6,
      fogFadeOutRangeY: 12,
      fogDensityMultiplier: 0.126,
      postDensityMultiplier: 1.2,
      halfRes: true,
      ...{
        [GraphicsQuality.Low]: { baseRaymarchStepCount: 58 },
        [GraphicsQuality.Medium]: { baseRaymarchStepCount: 80 },
        [GraphicsQuality.High]: { baseRaymarchStepCount: 110 },
      }[quality],
    });
    composer.addPass(volumetricPass);
    viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
  });

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -10,
      dashConfig: {
        enable: true,
        chargeConfig: { curCharges: curDashCharges },
      },
    },
    debugPos: true,
    debugPlayerKinematics: true,
    locations,
    legacyLights: false,
    customControlsEntries: [{ label: 'Reset', key: 'f', action: reset }],
  };
};
