import * as THREE from 'three';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { ParkourManager, partitionParkourObjects } from 'src/viz/parkour/ParkourManager.svelte';
import { Score, type ScoreThresholds } from 'src/viz/parkour/timeDisplayTypes';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildPylonsCheckpointMaterial } from 'src/viz/parkour/regions/pylons/materials';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { SkyStack, HorizonMode, gradientBackground } from 'src/viz/SkyStack';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';

export const processLoadedScene = (viz: Viz, loadedWorld: THREE.Group, vizConf: VizConfig): SceneConfig => {
  viz.camera.near = 2;
  viz.camera.far = 50_000;
  viz.camera.updateProjectionMatrix();

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
        pos: new THREE.Vector3(0, 63, 0),
        rot: new THREE.Vector3(-0.35, -Math.PI / 2, 0),
      },
    },
    scoreThresholds,
    undefined,
    'city',
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
        // slopeSlide: {
        //   minAngle: 0.4,
        //   maxSpeed: 80,
        // },
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

  const skyStack = new SkyStack(
    viz,
    {
      horizonOffset: -0.038,
      horizonBlend: 0.03,
      layers: [],
      background: gradientBackground({
        stops: [
          // { position: 0.0, color: 0x8c9db1 },
          // { position: 0.489, color: 0xaabac9 },
          // { position: 0.676, color: 0xbfc4c6 },
          // { position: 0.768, color: 0xc8c2bb },
          // { position: 0.856, color: 0xcbb5a5 },
          // { position: 0.905, color: 0xc5a597 },
          // { position: 0.944, color: 0xb29790 },
          // { position: 1.0, color: 0x828283 },
          { position: 0.0, color: 0xc37790 },
          { position: 0.348624, color: 0xc16f86 },
          { position: 0.513761, color: 0xa56a83 },
          { position: 0.876147, color: 0x895b6a },
          { position: 1.0, color: 0x825461 },
        ]
          .map(({ position, color }) => ({ position: 1 - position, color }))
          .reverse(),
        horizonMode: HorizonMode.SolidBelow,
        belowColor: 0x0,
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
    toneMapping: { mode: 'agx', exposure: 1 },
    autoUpdateShadowMap: false,
    emissiveBypass: true,
    skyBypassTonemap: true,
    skyStack,
    emissiveBloom:
      vizConf.graphics.quality > GraphicsQuality.Low
        ? { intensity: 6.0, levels: 3, luminanceThreshold: 0.02, radius: 0.45, luminanceSoftKnee: 0.02 }
        : null,
    fogShader: `vec4 getFogEffect(vec3 worldPos, vec3 cameraPos, vec3 playerPos, float depth, float curTimeSeconds) {
          // Sky pixels sit at the far plane; skip fogging so the gradient sky is untouched.
          if (depth >= 0.9999) {
            return vec4(0.0);
          }
          float yActivation = smoothstep(-60., -50., worldPos.y);
          float distToPlayer = distance(worldPos.xz, playerPos.xz) + 0.01 * abs(worldPos.y - playerPos.y);
          float fogFactor = smoothstep(140., 2310., distToPlayer) * yActivation * 0.83;
          return vec4(vec3(0.1, 0.035, 0.04) * 0.5, fogFactor);
        }`,
    addMiddlePasses: (composer, viz, quality) => {
      const qualityParams = {
        [GraphicsQuality.Low]: {
          baseRaymarchStepCount: 40,
          octaveCount: 3,
          renderScale: 0.25,
          fogFadeOutRangeY: 8,
          fogFadeOutPow: 1.6,
          globalScale: 1.4,
          noisePow: 1.5,
          noiseBias: 0.5,
          jbuExtent: 1,
          jbuSpatialSigma: 1.3,
          jbuDepthSigma: 0.05,
        },
        [GraphicsQuality.Medium]: { baseRaymarchStepCount: 30 },
        [GraphicsQuality.High]: { baseRaymarchStepCount: 60 },
      }[quality];
      const volumetricPass = new VolumetricPass(viz.scene, viz.camera as THREE.PerspectiveCamera, {
        fogMinY: -90,
        fogMaxY: -40,
        fogColorHighDensity: new THREE.Vector3(0.024, 0.024, 0.01).multiplyScalar(0.3),
        fogColorLowDensity: new THREE.Vector3(0.035, 0.03, 0.04).multiplyScalar(0.8),
        ambientLightColor: new THREE.Color(0x5d4444),
        ambientLightIntensity: 2.2,
        heightFogStartY: -90,
        heightFogEndY: -55,
        heightFogFactor: 0.54,
        maxRayLength: 1000,
        minStepLength: 0.1,
        noiseBias: 0.1,
        noisePow: 2.4,
        fogFadeOutRangeY: 38,
        fogFadeOutPow: 0.6,
        fogDensityMultiplier: 0.82,
        postDensityMultiplier: 1.7,
        noiseMovementPerSecond: new THREE.Vector2(-2.3, 1.3),
        globalScale: 1,
        halfRes: quality <= GraphicsQuality.Medium,
        ...qualityParams,
      });
      composer.addPass(volumetricPass);
      viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));

      if (vizConf.graphics.quality > GraphicsQuality.Low) {
        const n8aoPass = new N8AOPostPass(
          viz.scene,
          viz.camera,
          viz.renderer.domElement.width,
          viz.renderer.domElement.height
        );
        composer.addPass(n8aoPass);
        n8aoPass.gammaCorrection = false;
        n8aoPass.enabled = vizConf.graphics.quality > GraphicsQuality.Medium;
        n8aoPass.configuration.intensity = 2;
        n8aoPass.configuration.aoRadius = 5;
        n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Medium;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Low',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'High',
          }[vizConf.graphics.quality]
        );
      }
    },
  });

  const handle = viz.levelLoadHandle!;

  handle.setMaterialFactories({
    checkpoint: viz => {
      const mat = buildPylonsCheckpointMaterial(viz);
      return { material: mat, onAssigned: mesh => mat.setMesh(mesh) };
    },
  });

  handle.parkourObjects.then(parkourObjs => {
    const { checkpointMeshes, dashTokens } = partitionParkourObjects(parkourObjs);
    pkManager.setMaterials(undefined, { checkpointMeshes, dashTokens });
  });

  return pkManager.buildSceneConfig();
};
