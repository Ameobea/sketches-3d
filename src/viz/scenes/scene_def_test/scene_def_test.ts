import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { initLevelEditor } from 'src/viz/levelDef/LevelEditor.svelte';
import { loadLevelDef } from 'src/viz/levelDef/loadLevelDef';
import type { LevelDef } from 'src/viz/levelDef/types';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';

export const processLoadedScene = (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig,
  levelDef: LevelDef
): SceneConfig => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.2);
  viz.scene.add(ambientLight);

  const sunLight = new THREE.DirectionalLight(0xffffff, 2.0);
  sunLight.position.set(40, 80, 40);
  sunLight.castShadow = true;

  const shadowMapSize = {
    [GraphicsQuality.Low]: 1024,
    [GraphicsQuality.Medium]: 2048,
    [GraphicsQuality.High]: 4096,
  }[vizConf.graphics.quality];
  sunLight.shadow.mapSize.width = shadowMapSize;
  sunLight.shadow.mapSize.height = shadowMapSize;
  sunLight.shadow.camera.near = 0.1;
  sunLight.shadow.camera.far = 300;
  sunLight.shadow.camera.left = -80;
  sunLight.shadow.camera.right = 80;
  sunLight.shadow.camera.top = 80;
  sunLight.shadow.camera.bottom = -80;
  viz.scene.add(sunLight);

  const handle = loadLevelDef(viz, loadedWorld, levelDef);

  handle.objects.then(objects => {
    initLevelEditor(
      viz,
      objects,
      'scene_def_test',
      handle.prototypes,
      handle.builtMaterials,
      handle.loadedTextures,
      levelDef
    );
  });

  const playerHeight = 3.5;
  const playerRadius = 1;
  const playerMesh = new THREE.Mesh(
    new THREE.CapsuleGeometry(playerRadius, playerHeight, 16, 16),
    buildCustomShader({
      color: new THREE.Color(0x8d3d9f),
      metalness: 0.18,
      roughness: 0.82,
    })
  );
  playerMesh.castShadow = false;
  playerMesh.receiveShadow = true;

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
    {
      spawn: {
        pos: new THREE.Vector3(0, 3, 8),
        rot: new THREE.Vector3(0, Math.PI, 0),
      },
    },
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
      gravity: 220,
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
        moveSpeed: { onGround: 12.5, inAir: 18 },
        jumpVelocity: 76,
        terminalVelocity: 180,
        dashConfig: {
          chargeConfig: { curCharges: rwritable(Infinity) },
          dashMagnitude: 16,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
        },
        coyoteTimeSeconds: 0.135,
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.99999995, 0.99999995, 0.99999995),
        maxSlopeRadians: 1.3,
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

  initPylonsPostprocessing(viz, vizConf, false, { toneMapping: { mode: 'agx', exposure: 0.9 } });

  return pkManager.buildSceneConfig();
};
