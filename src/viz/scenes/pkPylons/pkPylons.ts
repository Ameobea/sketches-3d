import { get, writable, type Writable } from 'svelte/store';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import type { SceneConfig } from '..';
import { CollectablesCtx, initCollectables } from './collectables';
import { initDashTokens } from './DashToken';
import TimerDisplay from './TimerDisplay.svelte';
import TimeDisplay, { Score, type ScoreThresholds } from './TimeDisplay.svelte';
import { buildMaterials } from './materials';

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
      fogMinY: -140,
      fogMaxY: -5,
      fogColorHighDensity: new THREE.Vector3(0.32, 0.35, 0.38),
      fogColorLowDensity: new THREE.Vector3(0.9, 0.9, 0.9),
      ambientLightColor: new THREE.Color(0xffffff),
      ambientLightIntensity: 1.2,
      heightFogStartY: -140,
      heightFogEndY: -125,
      heightFogFactor: 0.14,
      maxRayLength: 1000,
      minStepLength: 0.1,
      noiseBias: 0.1,
      noisePow: 3.1,
      fogFadeOutRangeY: 32,
      fogFadeOutPow: 0.6,
      fogDensityMultiplier: 0.22,
      postDensityMultiplier: 1.4,
      noiseMovementPerSecond: new THREE.Vector2(4.1, 4.1),
      globalScale: 1,
      halfRes: true,
      compositor: { edgeRadius: 4, edgeStrength: 2 },
      ...{
        [GraphicsQuality.Low]: { baseRaymarchStepCount: 88 },
        [GraphicsQuality.Medium]: { baseRaymarchStepCount: 130 },
        [GraphicsQuality.High]: { baseRaymarchStepCount: 240 },
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
