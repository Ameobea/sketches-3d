import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildPylonsCheckpointMaterial } from 'src/viz/parkour/regions/pylons/materials';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';

const collectMeshes = (obj: THREE.Object3D): THREE.Mesh[] => {
  const out: THREE.Mesh[] = [];
  obj.traverse(child => {
    if (child instanceof THREE.Mesh) {
      out.push(child);
    }
  });
  return out;
};

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  // // non-shadow-casting faint red point light that follows the player, sitting 5 units above them
  // const playerLight = new THREE.PointLight(0x753c3c, 3.5, 150, 0.2);
  // playerLight.castShadow = false;
  // viz.scene.add(playerLight);
  // viz.registerBeforeRenderCb(() => {
  //   const playerPos = viz.fpCtx?.playerController.getPosition();
  //   if (!playerPos) {
  //     return;
  //   }
  //   playerLight.position.set(playerPos.x(), playerPos.y() + 5, playerPos.z());
  // });

  const playerHeight = 5;
  const playerRadius = 1.5;
  const playerMesh = new THREE.Mesh(
    new THREE.CapsuleGeometry(playerRadius, playerHeight, 16, 16),
    buildCustomShader(
      {
        color: new THREE.Color(0x8d3d9f),
        metalness: 0.18,
        roughness: 0.82,
      },
      {},
      { noOcclusion: true }
    )
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
        pos: new THREE.Vector3(-48, 3, -14),
        rot: new THREE.Vector3(-0.35, -Math.PI / 2, 0),
      },
    },
    scoreThresholds,
    undefined,
    'factory',
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
        moveSpeed: { onGround: 18.9, inAir: 21.6 },
        jumpVelocity: 76,
        terminalVelocity: 180,
        maxPenetrationDepth: 0.008,
        dashConfig: {
          chargeConfig: { curCharges: rwritable(0) },
          dashMagnitude: 16,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
        },
        coyoteTimeSeconds: 0.135,
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.99999995, 0.99999995, 0.99999995),
        maxSlopeRadians: 1.4,
        oobYThreshold: -200,
        slopeSlide: {
          minAngle: 1.1,
          maxSpeed: 80,
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

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    toneMapping: { mode: 'agx', exposure: 1 },
    autoUpdateShadowMap: true,
    emissiveBypass: true,
    emissiveBloom:
      vizConf.graphics.quality > GraphicsQuality.Low
        ? { intensity: 2.5, levels: 2, luminanceThreshold: 0.08, radius: 0.1 }
        : null,
    fogShader: `vec4 getFogEffect(vec3 worldPos, vec3 cameraPos, vec3 playerPos, float depth, float curTimeSeconds) {
          float distToPlayer = distance(worldPos.xz, playerPos.xz) + 0.01 * abs(worldPos.y - playerPos.y);
          float fogFactor = smoothstep(80., 290., distToPlayer);
          return vec4(vec3(0.0002, 0.0002, 0.0002), fogFactor);
        }`,
  });

  const handle = viz.levelLoadHandle!;

  handle.setMaterialFactories({
    checkpoint: viz => {
      const mat = buildPylonsCheckpointMaterial(viz);
      return { material: mat, onAssigned: mesh => mat.setMesh(mesh) };
    },
  });

  handle.parkourObjects.then(parkourObjs => {
    const checkpointMeshes = parkourObjs.flatMap(obj => collectMeshes(obj.object));
    pkManager.setMaterials(
      {
        dashToken: {
          core: new THREE.MeshStandardMaterial({ color: 0x9effe1, emissive: 0x1a322e, roughness: 0.4 }),
          ring: new THREE.MeshStandardMaterial({ color: 0xffd464, emissive: 0x36290a, roughness: 0.3 }),
        },
      },
      { checkpointMeshes }
    );
  });

  return pkManager.buildSceneConfig();
};
