import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { ParkourManager, partitionParkourObjects } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import type { SceneConfig } from '..';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { initWebSynth } from 'src/viz/webSynth';
import { delay } from 'src/viz/util/util';
import { gradientBackground, HorizonMode, SkyStack, waveOceanLayer } from 'src/viz/SkyStack';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 3, -24),
    rot: new THREE.Vector3(0, Math.PI, 0),
  },
};

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  const playerHeight = 2.5;
  const playerRadius = 0.7;
  const playerMesh = new THREE.Mesh(
    new THREE.CapsuleGeometry(playerRadius, playerHeight, 16, 16),
    buildCustomShader({
      color: new THREE.Color(0xad6dcf),
      metalness: 0.18,
      roughness: 0.82,
    })
  );
  playerMesh.castShadow = false;
  playerMesh.receiveShadow = false;

  const scoreThresholds: ScoreThresholds = {
    [Score.SPlus]: Infinity,
    [Score.S]: Infinity,
    [Score.A]: Infinity,
    [Score.B]: Infinity,
  };
  viz.sfxManager.registerSfxDefs({
    engage: {
      url: 'https://i.ameo.link/dsj.ogg',
      playbackRate: [3.65, 3.7],
      gain: 0.5,
    },
    disengage: {
      url: 'https://i.ameo.link/dsj.ogg',
      playbackRate: [3.65, 3.7],
      gain: 0.5,
      reverse: true,
    },
    ceramic_click: {
      url: 'https://i.ameo.link/dsk.ogg',
      playbackRate: 3,
      filter: { type: 'hp', freq: [7920, 8000], q: 4 },
      gain: 0.3,
    },
    ceramic_click_reverse: {
      url: 'https://i.ameo.link/dsk.ogg',
      playbackRate: 3,
      filter: { type: 'hp', freq: [7920, 8000], q: 4 },
      reverse: true,
      gain: 0.3,
    },
    boosted_jump: {
      url: 'https://i.ameo.link/dml.ogg',
      playbackRate: [2.25, 2.3],
      filter: { type: 'hp', freq: [5000, 6000], q: 1 },
      gain: 0.3,
    },
    waves: {
      url: 'https://i.ameo.link/dun.ogg',
      gain: 0.3,
      filter: { type: 'hp', freq: 200, q: 1 },
    },
  });
  viz.sfxManager.loadSfx('engage');
  viz.sfxManager.loadSfx('disengage');
  viz.sfxManager.loadSfx('ceramic_click');
  viz.sfxManager.loadSfx('ceramic_click_reverse');
  viz.sfxManager.loadSfx('boosted_jump');
  viz.sfxManager.loadSfx('waves').then(() => {
    // TODO: this should be a custom attenuation pattern which just works off of y position
    viz.sfxManager.playSpatialLoop('waves', { pos: [0, 0, 0], rolloff: 0.5, refDistance: 1000, xfade: 0.05 });
  });

  const pkManager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    locations,
    scoreThresholds,
    undefined,
    'jump_pad_speedup_test',
    true,
    {
      gravity: 200,
      gravityShaping: {
        riseMultiplier: 1.0,
        apexMultiplier: 0.6,
        fallMultiplier: 1.2,
        apexThreshold: 4.0,
        kneeWidth: 0.1,
      },
      player: {
        playerColliderShape: 'capsule',
        mesh: playerMesh,
        colliderSize: { height: playerHeight, radius: playerRadius },
        playerShadow: { radius: playerRadius, intensity: 0.85 },
        moveSpeed: { onGround: 18.9, inAir: 21.6 },
        jumpVelocity: 66,
        terminalVelocity: 180,
        dashConfig: {
          enable: true,
          chargeConfig: { curCharges: rwritable(0) },
          dashMagnitude: 60,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
          directionMode: 'vertical-up',
          cancelFallVelocity: true,
          verticalUseJump: false,
        },
        coyoteTimeSeconds: 0.125,
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.99999995, 0.99999995, 0.99999995),
        maxSlopeRadians: 1.4,
        boostArmLeniencySeconds: 0.1,
        externalVelocityAirIdleDampingFactor: new THREE.Vector3(0.92, 0.92, 0.92),
        oobYThreshold: -20,
      },
      sfx: {
        boost: {
          contactStartSfx: 'ceramic_click',
          contactEndSfx: 'ceramic_click_reverse',
          startSfx: 'engage',
          endSfx: 'disengage',
          boostedJumpSfx: 'boosted_jump',
        },
      },
      viewMode: {
        type: 'thirdPerson',
        distance: 15,
        cameraFOV: 75,
        zoomEnabled: true,
        maxZoomDistance: 40,
        zoomSpeed: 5,
      },
    }
  );

  const sceneConfig = pkManager.buildSceneConfig();

  viz.levelLoadHandle?.setSceneRuntime(pkManager.runtime, 'jump_pad_speedup_test');

  viz.levelLoadHandle?.parkourObjects.then(parkourObjs => {
    const { checkpointMeshes, dashTokens } = partitionParkourObjects(parkourObjs);
    pkManager.setMaterials(
      {
        checkpoint: new THREE.MeshStandardMaterial({ color: 0x80f0ff, emissive: 0x173845, roughness: 0.32 }),
      },
      { checkpointMeshes, dashTokens }
    );
  });

  const skyStack = new SkyStack(
    viz,
    {
      horizonOffset: -0.038,
      horizonBlend: 0.02,
      layers: [
        waveOceanLayer({
          // debugMode: 1,
          id: 'waveOcean',
          zIndex: 5,
          maxSteps: {
            [GraphicsQuality.Low]: 16,
            [GraphicsQuality.Medium]: 21,
            [GraphicsQuality.High]: 36,
          }[vizConf.graphics.quality],
          lodBias: {
            [GraphicsQuality.Low]: 1.2,
            [GraphicsQuality.Medium]: 0.5,
            [GraphicsQuality.High]: 0.22,
          }[vizConf.graphics.quality],
          oversample: vizConf.graphics.quality > GraphicsQuality.Medium ? 3 : false,
        }),
      ],
      background: gradientBackground({
        stops: [
          ...[
            { col: [3, 137, 237], t: 0 },
            { col: [6, 165, 245], t: 0.464151 },
            { col: [22, 188, 245], t: 0.732075 },
            { col: [48, 193, 245], t: 0.860377 },
            { col: [63, 185, 248], t: 1.0 },
          ].map(({ t, col }) => ({
            position: t,
            color: new THREE.Color(col[0] / 255, col[1] / 255, col[2] / 255),
          })),
        ]
          .map(({ position, color }) => ({ position: 1 - position, color }))
          .reverse(),
        horizonMode: HorizonMode.SolidBelow,
        belowColor: 0x1f5a66,
        lutResolution: {
          [GraphicsQuality.Low]: 32,
          [GraphicsQuality.Medium]: 64,
          [GraphicsQuality.High]: 128,
        }[vizConf.graphics.quality],
      }),
    },
    viz.renderer.domElement.width,
    viz.renderer.domElement.height
  );
  viz.registerBeforeRenderCb(curTimeSeconds => skyStack.setTime(curTimeSeconds));

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    emissiveBloom: {},
    emissiveBypass: true,
    // autoUpdateShadowMap: true,
    skyStack,
    pomExitBuffers: true,
    addMiddlePasses: (composer, viz, quality) => {
      let n8aoPass: typeof N8AOPostPass | null = null;
      if (quality > GraphicsQuality.Low) {
        n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        n8aoPass.gammaCorrection = false;
        n8aoPass.configuration.intensity = 4.6;
        n8aoPass.configuration.aoRadius = 3.5;
        n8aoPass.configuration.halfRes = quality <= GraphicsQuality.Medium;
        n8aoPass.configuration.denoiseIterations = 1;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Performance',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'Medium',
          }[quality]
        );
        composer.addPass(n8aoPass, 3);
        n8aoPass.autoDetectTransparency = false;
        n8aoPass.configuration.transparencyAware = false;
      }
    },
  });

  const startMusic = () =>
    initWebSynth({ compositionIDToLoad: 184 }).then(async ctx => {
      await delay(1200);

      ctx.setGlobalBpm(180);
      ctx.startAll();
    });
  if (typeof requestIdleCallback === 'function') {
    requestIdleCallback(startMusic, { timeout: 3000 });
  } else {
    setTimeout(startMusic, 2000);
  }

  return sceneConfig;
};
