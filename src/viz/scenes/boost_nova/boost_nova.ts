import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import type { SceneConfig } from '..';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { initWebSynth } from 'src/viz/webSynth';
import { delay } from 'src/viz/util/util';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 3, -24),
    rot: new THREE.Vector3(0, Math.PI, 0),
  },
};

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  const dashToken = new THREE.Group();
  dashToken.name = 'dash_token';
  dashToken.visible = false;
  const dashTokenCore = new THREE.Mesh(
    new THREE.SphereGeometry(0.45, 12, 12),
    new THREE.MeshStandardMaterial()
  );
  dashTokenCore.name = 'core';
  const dashTokenRing = new THREE.Mesh(
    new THREE.TorusGeometry(0.72, 0.1, 8, 24),
    new THREE.MeshStandardMaterial()
  );
  dashTokenRing.name = 'ring';
  dashToken.add(dashTokenCore, dashTokenRing);
  loadedWorld.add(dashToken);

  const playerHeight = 2.2;
  const playerRadius = 0.5;
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
  });
  const pkManager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    locations,
    scoreThresholds,
    {
      dashToken: {
        core: new THREE.MeshStandardMaterial({ color: 0x9effe1, emissive: 0x1a322e, roughness: 0.4 }),
        ring: new THREE.MeshStandardMaterial({ color: 0xffd464, emissive: 0x36290a, roughness: 0.3 }),
      },
      checkpoint: new THREE.MeshStandardMaterial({ color: 0x80f0ff, emissive: 0x173845, roughness: 0.32 }),
    },
    'jump_pad_speedup_test',
    true,
    {
      gravity: 80,
      gravityShaping: {
        riseMultiplier: 1.0,
        apexMultiplier: 1.4,
        fallMultiplier: 1.5,
        apexThreshold: 4.0,
        kneeWidth: 0.1,
      },
      player: {
        playerColliderShape: 'capsule',
        mesh: playerMesh,
        moveSpeed: { onGround: 15, inAir: 20 },
        jumpVelocity: 30,
        terminalVelocity: 80,
        dashConfig: {
          enable: true,
          chargeConfig: { curCharges: rwritable(Infinity) },
          dashMagnitude: 40,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
          directionMode: 'vertical-up',
          cancelFallVelocity: true,
          verticalUseJump: false,
        },
        coyoteTimeSeconds: 0.1,
        boostArmLeniencySeconds: 0.1,
        externalVelocityAirIdleDampingFactor: new THREE.Vector3(0.92, 0.92, 0.92),
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
        maxZoomDistance: 50,
      },
    }
  );

  const sceneConfig = pkManager.buildSceneConfig();

  viz.levelLoadHandle?.setSceneRuntime(pkManager.runtime, 'jump_pad_speedup_test');

  viz.levelLoadHandle?.complete.then(() => {
    const mat = viz.levelLoadHandle?.builtMaterials.get('__ASSETS__/materials/procedural/raised_tiles');
    if (mat instanceof THREE.ShaderMaterial && mat.uniforms.u_heightPhase) {
      let heightPhase = 0;
      viz.registerBeforeRenderCb((curTimeSeconds, tDiffSeconds) => {
        const rate = THREE.MathUtils.smoothstep(Math.sin(curTimeSeconds * 4), 0.4, 1);
        heightPhase += rate * 2 * tDiffSeconds;
        mat.uniforms.u_heightPhase.value = heightPhase;
      });
    }
  });

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    // autoUpdateShadowMap: true,
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

  initWebSynth({ compositionIDToLoad: 184 }).then(async ctx => {
    await delay(1200);

    ctx.setGlobalBpm(180);
    ctx.startAll();
  });

  return sceneConfig;
};
