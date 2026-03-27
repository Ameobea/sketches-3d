import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { generateParkourPlatforms, jumpSimParamsFromSceneConfig } from 'src/viz/parkour/platformGen';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import type { SceneConfig } from '..';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildCustomShader } from 'src/viz/shaders/customShader';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 3, -24),
    rot: new THREE.Vector3(0, Math.PI, 0),
  },
};

const addPlatform = (
  viz: Viz,
  pos: THREE.Vector3,
  size: THREE.Vector3,
  material: THREE.Material
): THREE.Mesh<THREE.BoxGeometry, THREE.Material> => {
  const platform = new THREE.Mesh(new THREE.BoxGeometry(size.x, size.y, size.z), material);
  platform.position.copy(pos);
  platform.receiveShadow = true;
  platform.castShadow = true;
  viz.scene.add(platform);
  viz.fpCtx!.addTriMesh(platform);
  return platform;
};

const addZoneVisual = (viz: Viz, pos: THREE.Vector3, halfExtents: THREE.Vector3, color: number) => {
  const visual = new THREE.Mesh(
    new THREE.BoxGeometry(halfExtents.x * 2, halfExtents.y * 2, halfExtents.z * 2),
    new THREE.MeshBasicMaterial({ color, transparent: true, opacity: 0.2, wireframe: true })
  );
  visual.position.copy(pos);
  viz.scene.add(visual);
};

const initLevel = (viz: Viz, sceneConfig: SceneConfig) => {
  const fpCtx = viz.fpCtx!;

  const floorMat = new THREE.MeshStandardMaterial({ color: 0x515763, roughness: 0.95, metalness: 0.04 });
  const platformMat = new THREE.MeshStandardMaterial({ color: 0x6f7f96, roughness: 0.85, metalness: 0.06 });
  const jumpPadMat = new THREE.MeshStandardMaterial({
    color: 0x31a0ff,
    emissive: 0x14334e,
    roughness: 0.38,
    metalness: 0.12,
  });

  addPlatform(viz, new THREE.Vector3(0, -1, 20), new THREE.Vector3(120, 2, 140), floorMat);
  addPlatform(viz, new THREE.Vector3(0, 10, 68), new THREE.Vector3(32, 2, 30), platformMat);
  addPlatform(viz, new THREE.Vector3(42, 18, 100), new THREE.Vector3(28, 2, 28), platformMat);

  addPlatform(viz, new THREE.Vector3(0, 0.25, 18), new THREE.Vector3(6, 0.5, 6), jumpPadMat);
  addPlatform(viz, new THREE.Vector3(0, 11.25, 68), new THREE.Vector3(6, 0.5, 6), jumpPadMat);

  fpCtx.addJumpPad(
    {
      type: 'box',
      pos: new THREE.Vector3(0, 1.1, 18),
      halfExtents: new THREE.Vector3(3.2, 1.2, 3.2),
    },
    {
      baseImpulse: 106,
      speedScaling: 0.28,
      cooldownSeconds: 0.15,
      direction: new THREE.Vector3(0, 1, 0),
    }
  );

  fpCtx.addJumpPad(
    {
      type: 'box',
      pos: new THREE.Vector3(0, 12.1, 68),
      halfExtents: new THREE.Vector3(3.2, 1.2, 3.2),
    },
    {
      baseImpulse: 110,
      speedScaling: 0.2,
      cooldownSeconds: 0.15,
      direction: new THREE.Vector3(0.45, 1, 0.15).normalize(),
    }
  );

  fpCtx.addBoostZone(
    {
      type: 'box',
      pos: new THREE.Vector3(0, 1.2, 6),
      halfExtents: new THREE.Vector3(5, 1.2, 12),
    },
    {
      strength: 26,
      directionalBias: 1.0,
      direction: new THREE.Vector3(0, 0, 1),
    }
  );

  fpCtx.addBoostZone(
    {
      type: 'box',
      pos: new THREE.Vector3(16, 12.2, 78),
      halfExtents: new THREE.Vector3(14, 1.2, 6),
    },
    {
      strength: 24,
      directionalBias: 0.9,
      direction: new THREE.Vector3(1, 0, 0.35).normalize(),
    }
  );

  addZoneVisual(viz, new THREE.Vector3(0, 1.2, 6), new THREE.Vector3(5, 1.2, 12), 0x88ff88);
  addZoneVisual(viz, new THREE.Vector3(16, 12.2, 78), new THREE.Vector3(14, 1.2, 6), 0x88ff88);
  addZoneVisual(viz, new THREE.Vector3(0, 1.1, 18), new THREE.Vector3(3.2, 1.2, 3.2), 0x5db9ff);
  addZoneVisual(viz, new THREE.Vector3(0, 12.1, 68), new THREE.Vector3(3.2, 1.2, 3.2), 0x5db9ff);

  const spline = new THREE.CatmullRomCurve3([
    new THREE.Vector3(0, 1, -22),
    new THREE.Vector3(-30, 8, -5),
    new THREE.Vector3(-52, 16, 20),
    new THREE.Vector3(-0, 24, 55),
    new THREE.Vector3(0, 32, 82),
  ]);
  // const spline = new THREE.CatmullRomCurve3([
  //   new THREE.Vector3(0, 1, -22),
  //   new THREE.Vector3(-30, 5, -5),
  //   new THREE.Vector3(-42, 10, 20),
  //   new THREE.Vector3(-30, 15, 45),
  //   new THREE.Vector3(0, 20, 82),
  // ]);

  const jumpParams = jumpSimParamsFromSceneConfig(sceneConfig);

  const positions = generateParkourPlatforms(
    spline,
    { ...jumpParams, initialExternalVelocity: 10 * 1.28 },
    1.03
  );

  const platHeight = 0.4;
  const genMat = new THREE.MeshStandardMaterial({ color: 0x44aa66, roughness: 0.7, metalness: 0.1 });

  for (const pos of positions) {
    const mesh = new THREE.Mesh(new THREE.CylinderGeometry(3, 3, platHeight, 24), genMat);
    mesh.position.set(pos.x, pos.y - platHeight / 2, pos.z);
    mesh.receiveShadow = true;
    mesh.castShadow = true;
    viz.scene.add(mesh);
    fpCtx.addTriMesh(mesh);
  }
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
  playerMesh.castShadow = true;
  playerMesh.receiveShadow = true;

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
          chargeConfig: { curCharges: rwritable(Infinity) },
          dashMagnitude: 10,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
        },
        coyoteTimeSeconds: 0.1,
      },
      viewMode: {
        type: 'thirdPerson',
        distance: 18,
        cameraFOV: 75,
      },
    }
  );

  const sceneConfig = pkManager.buildSceneConfig();

  const ambientLight = new THREE.AmbientLight(0xffffff, 0.62);
  viz.scene.add(ambientLight);

  const sunLight = new THREE.DirectionalLight(0xffffff, 2.6);
  sunLight.position.set(70, 105, -45);
  sunLight.castShadow = true;
  const shadowMapSize = {
    [GraphicsQuality.Low]: 1024,
    [GraphicsQuality.Medium]: 2048,
    [GraphicsQuality.High]: 4096,
  }[vizConf.graphics.quality];
  sunLight.shadow.mapSize.width = shadowMapSize;
  sunLight.shadow.mapSize.height = shadowMapSize;
  sunLight.shadow.camera.near = 0.1;
  sunLight.shadow.camera.far = 250;
  sunLight.shadow.camera.left = -90;
  sunLight.shadow.camera.right = 90;
  sunLight.shadow.camera.top = 90;
  sunLight.shadow.camera.bottom = -90;
  sunLight.shadow.camera.updateProjectionMatrix();
  viz.scene.add(sunLight);

  viz.collisionWorldLoadedCbs.push(() => initLevel(viz, sceneConfig));

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    autoUpdateShadowMap: true,
  });

  return sceneConfig;
};
