import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { initPylonsPostprocessing } from '../pkPylons/postprocessing';
import { loadTexture } from 'src/viz/textureLoading';
import { buildPylonsCheckpointMaterial } from 'src/viz/parkour/regions/pylons/materials';

const collectMeshes = (obj: THREE.Object3D): THREE.Mesh[] => {
  const out: THREE.Mesh[] = [];
  obj.traverse(child => {
    if (child instanceof THREE.Mesh) out.push(child);
  });
  return out;
};

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 0.4);
  viz.scene.add(ambientLight);

  const sunLight = new THREE.DirectionalLight(0xffffff, 1.7);
  sunLight.position.set(40, 80, 40);
  sunLight.castShadow = true;

  const loader = new THREE.ImageBitmapLoader();
  loadTexture(loader, 'https://i.ameo.link/dlz.avif', {
    colorSpace: THREE.SRGBColorSpace,
    mapping: THREE.EquirectangularRefractionMapping,
  }).then(texture => {
    viz.scene.background = texture;
  });

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

  const playerHeight = 3.5;
  const playerRadius = 1;
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
    [Score.SPlus]: 24.0,
    [Score.S]: 26,
    [Score.A]: 29,
    [Score.B]: 24,
  };
  const pkManager = new ParkourManager(
    viz,
    loadedWorld,
    vizConf,
    {
      spawn: {
        pos: new THREE.Vector3(0, 998 , 8),
        rot: new THREE.Vector3(0, Math.PI, 0),
      },
    },
    scoreThresholds,
    undefined,
    'holes',
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
        moveSpeed: { onGround: 15.75, inAir: 18 },
        jumpVelocity: 76,
        terminalVelocity: 180,
        dashConfig: {
          chargeConfig: { curCharges: rwritable(0) },
          dashMagnitude: 16,
          useExternalVelocity: true,
          minDashDelaySeconds: 0.3,
        },
        coyoteTimeSeconds: 0.135,
        externalVelocityGroundDampingFactor: new THREE.Vector3(0.99999995, 0.99999995, 0.99999995),
        maxSlopeRadians: 1.3,
        oobYThreshold: -50,
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

  initPylonsPostprocessing(
    viz,
    vizConf,
    false,
    {
      toneMapping: { mode: 'agx', exposure: 0.9 },
      useDepthPrePass: true,
      emissiveBypass: true,
      emissiveBypassAmbientIntensity: vizConf.graphics.quality > GraphicsQuality.Low ? 2.8 : 3,
      emissiveBloom:
        vizConf.graphics.quality > GraphicsQuality.Low
          ? { luminanceThreshold: 1.1, luminanceSmoothing: 0 }
          : null,
    },
    {
      fogColorHighDensity: new THREE.Vector3(0.12, 0.15, 0.14).multiplyScalar(0.2),
      fogColorLowDensity: new THREE.Vector3(0.15, 0.2, 0.25).multiplyScalar(0.7),
      ...(vizConf.graphics.quality >= GraphicsQuality.High
        ? { shadowLight: sunLight, shadowIntensity: 0.75, shadowBias: 0.05 }
        : {}),
    }
  );

  const handle = viz.levelLoadHandle!;

  // Register the factory for the checkpoint material so the level def system
  // builds and assigns it to the win-zone mesh when it's placed.
  handle.setMaterialFactories({
    checkpoint: viz => {
      const mat = buildPylonsCheckpointMaterial(viz);
      return { material: mat, onAssigned: mesh => mat.setMesh(mesh) };
    },
  });

  // Set up parkour collectables once level objects are placed, passing the
  // win-zone mesh directly instead of relying on name-based traversal.
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
