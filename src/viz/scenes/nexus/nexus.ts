import * as THREE from 'three';
import { goto } from '$app/navigation';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { BlendFunction, BloomEffect, EffectPass, ToneMappingEffect, ToneMappingMode } from 'postprocessing';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import {
  buildGrayFossilRockMaterial,
  GrayFossilRockTextures,
} from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';
import { createSignboard, type CreateSignboardArgs } from 'src/viz/helpers/signboardBuilder';
import { mix, smoothstep } from 'src/viz/util/util';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildCheckpointMaterial } from 'src/viz/materials/Checkpoint/CheckpointMaterial';
import { buildGrayStoneBricksFloorMaterial } from 'src/viz/materials/GrayStoneBricksFloor/GrayStoneBricksFloorMaterial';
import { getAmmoJS } from 'src/viz/collision';
import { MetricsAPI } from 'src/api/client';
import PlatformColorShader from './shaders/platform/color.frag?raw';
import PlatformRoughnessShader from './shaders/platform/roughness.frag?raw';

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const bgTextureP = (async () => {
    const bgImage = await loader.loadAsync('https://i.ameo.link/ccl.avif');
    const bgTexture = new THREE.Texture(bgImage);
    bgTexture.rotation = Math.PI;
    bgTexture.mapping = THREE.EquirectangularReflectionMapping;
    bgTexture.needsUpdate = true;
    return bgTexture;
  })();

  const platformTexsP = GrayFossilRockTextures.get(loader);
  const platformMatP = buildGrayFossilRockMaterial(loader);

  const [platformMat, bgTexture, { platformDiffuse, platformNormal }] = await Promise.all([
    platformMatP,
    bgTextureP,
    platformTexsP,
  ]);

  return { platformMat, bgTexture, platformDiffuse, platformNormal, loader };
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  // kick off request for physics engine wasm early.  This normally has to wait until after
  // this function returns, but we know we're going to be first-person so we can start it now
  getAmmoJS();

  const ambientLight = new THREE.AmbientLight(0xffffff, 6.4);
  viz.scene.add(ambientLight);

  const dirLight = new THREE.DirectionalLight(0xdde6f1, 3.2);
  dirLight.position.set(-160, 163, -80);
  dirLight.target.position.set(0, 0, 0);

  dirLight.castShadow = true;
  dirLight.shadow.mapSize.width = 2048 * 2;
  dirLight.shadow.mapSize.height = 2048 * 2;
  dirLight.shadow.radius = 4;
  dirLight.shadow.blurSamples = 16;
  viz.renderer.shadowMap.type = THREE.VSMShadowMap;
  dirLight.shadow.bias = -0.0001;

  dirLight.shadow.camera.near = 8;
  dirLight.shadow.camera.far = 300;
  dirLight.shadow.camera.left = -300;
  dirLight.shadow.camera.right = 380;
  dirLight.shadow.camera.top = 94;
  dirLight.shadow.camera.bottom = -140;

  // const shadowCameraHelper = new THREE.CameraHelper(dirLight.shadow.camera);
  // viz.scene.add(shadowCameraHelper);

  dirLight.shadow.camera.updateProjectionMatrix();
  dirLight.shadow.camera.updateMatrixWorld();

  viz.scene.add(dirLight);
  viz.scene.add(dirLight.target);

  const pointLightPos = new THREE.Vector3(-42.973, -20, -0.20153);
  const pointLightColor = new THREE.Color(0xbd6464);
  const pointLight = new THREE.PointLight(pointLightColor, 1, 0, 0);
  pointLight.castShadow = false;
  pointLight.position.copy(pointLightPos);
  viz.scene.add(pointLight);

  viz.registerBeforeRenderCb(() => {
    const pointLightActivation = 1 - smoothstep(-20, 0, viz.camera.position.y);
    pointLight.intensity = 13 * pointLightActivation;
    pointLight.position.x = mix(pointLightPos.x, viz.camera.position.x, 0.9);
    pointLight.position.z = mix(pointLightPos.z, viz.camera.position.z, 0.9);
  });

  const portalFrames: THREE.Mesh[] = [];
  const portals: THREE.Mesh[] = [];
  loadedWorld.traverse(obj => {
    if (!(obj instanceof THREE.Mesh)) {
      return;
    }

    if (obj.name.startsWith('portalframe')) {
      portalFrames.push(obj);
    } else if (obj.name.startsWith('portal')) {
      portals.push(obj);
    }
  });

  const EASY = [0.4, 0.7, 0.4] as [number, number, number];
  const NORMAL = [0.15, 0.51, 0.9] as [number, number, number];
  const HARD = [0.7, 0.5, 0.04] as [number, number, number];
  const VERY_HARD = [1.3, 0.3, 0.12] as [number, number, number];

  const PortalColorByName: Record<string, [number, number, number]> = {
    tutorial: EASY,
    stone: NORMAL,
    pylons: NORMAL,
    movementv2: HARD,
    plats: HARD,
    cornered: VERY_HARD,
  };

  for (const portal of portals) {
    portal.userData.nocollide = true;
    portal.material = buildCheckpointMaterial(viz, PortalColorByName[portal.name.split('_')[1]]);
    // it would be good to eventually be able to handle these transparent portals correctly so that the
    // volumetrics show up behind them, but that makes things very complicated with the depth pre-pass
    // and other render passes so isn't worth it for now
    // portal.material.depthWrite = false;
    portal.userData.noLight = true;

    if (!portal.name.includes('_')) {
      portal.visible = false;
    }
  }

  const { platformMat, bgTexture, platformDiffuse, platformNormal, loader } = await loadTextures();
  viz.scene.background = bgTexture;

  const platform = loadedWorld.getObjectByName('platform') as THREE.Mesh;
  platform.material = platformMat;

  const lowerPlatform = loadedWorld.getObjectByName('lower_platform') as THREE.Mesh;
  lowerPlatform.material = platformMat;
  buildGrayStoneBricksFloorMaterial(
    loader,
    {
      uvTransform: new THREE.Matrix3().scale(0.148, 0.148),
      metalness: 0.513,
      mapDisableDistance: null,
    },
    {
      colorShader: PlatformColorShader,
      roughnessShader: PlatformRoughnessShader,
    },
    { randomizeUVOffset: false }
  ).then(mat => {
    lowerPlatform.material = mat;
  });

  const spawnPlatformMat = buildCustomShader(
    {
      color: 0x474a4d,
      map: platformDiffuse,
      roughness: 0.9,
      metalness: 0.5,
      uvTransform: new THREE.Matrix3().scale(28.2073, 28.2073),
      normalMap: platformNormal,
      normalScale: 0.95,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
      ambientLightScale: 1.8,
    },
    {},
    { tileBreaking: { type: 'neyret', patchScale: 2 } }
  );

  const spawnPlatformDarkMat = buildCustomShader(
    {
      color: 0x474a4d,
      map: platformDiffuse,
      roughness: 0.9,
      metalness: 0.5,
      uvTransform: new THREE.Matrix3().scale(8.2073, 8.2073),
      normalMap: platformNormal,
      normalScale: 0.95,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
      ambientLightScale: 1.2,
    },
    {
      roughnessShader: `
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(ctx.diffuseColor.r * 27.5, 2.5) * 0.6;
  shinyness = clamp(shinyness, 0.0, 0.6);
  return 1. - shinyness;
}`,
    },
    { tileBreaking: { type: 'neyret', patchScale: 2 } }
  );

  const portalFrameMat = buildCustomShader(
    {
      color: 0x080808,
      uvTransform: new THREE.Matrix3().scale(0.24073, 0.24073),
      normalMap: platformNormal,
      normalScale: 0.75,
      normalMapType: THREE.TangentSpaceNormalMap,
      roughness: 0.7,
      metalness: 0.1,
    },
    {},
    { useGeneratedUVs: true, randomizeUVOffset: true }
  );

  const spawnPlatform = loadedWorld.getObjectByName('spawn_platform') as THREE.Mesh;
  spawnPlatform.material = spawnPlatformMat;

  const spawnPlatformDark = loadedWorld.getObjectByName('spawn_platform_dark') as THREE.Mesh;
  spawnPlatformDark.material = spawnPlatformDarkMat;

  const addPortalFrameSign = (portalFrame: THREE.Mesh, params: CreateSignboardArgs) => {
    const sign = createSignboard({
      width: 5.75,
      height: 3,
      fontSize: 56,
      align: 'center',
      canvasWidth: 400,
      canvasHeight: 200,
      textColor: '#888',
      ...params,
    });
    sign.position.copy(portalFrame.position);
    sign.rotation.copy(portalFrame.rotation);
    sign.rotation.y = sign.rotation.y + Math.PI;
    sign.position.y += 9.3;
    // move the sign forward wrt. the direction it's facing a bit
    sign.position.addScaledVector(portalFrame.getWorldDirection(new THREE.Vector3()), -2);
    viz.scene.add(sign);
  };

  const PortalDefs: Record<string, { scene: string; displayName: string }> = {
    tutorial: { scene: 'tutorial', displayName: 'TUTORIAL' },
    pylons: { scene: 'pk_pylons', displayName: 'PYLONS' },
    movementv2: { scene: 'movement_v2', displayName: 'MOVEMENT V2' },
    plats: { scene: 'plats', displayName: 'PLATS' },
    cornered: { scene: 'cornered', displayName: 'CORNERED' },
    stone: { scene: 'stone', displayName: 'STONE' },
    basalt: { scene: 'basalt', displayName: 'BASALT' },
    stronghold: { scene: 'stronghold', displayName: 'STRONGHOLD' },
    pinklights: { scene: 'pinklights', displayName: 'PINKLIGHTS' },
  };

  for (const portalFrame of portalFrames) {
    portalFrame.material = portalFrameMat;

    const portalKey = portalFrame.name.split('_')[1];
    const portal = PortalDefs[portalKey];
    if (portal) {
      addPortalFrameSign(portalFrame, { text: portal.displayName });
    }
  }

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    for (const portal of portals) {
      const key = portal.name.split('_')[1];
      const def = PortalDefs[key];

      if (def) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          MetricsAPI.recordPortalTravel(def.scene);
          goto(`/${def.scene}`, { keepFocus: true });
        });
      } else {
        portal.visible = false;
      }
    }
  });

  const lowerPortalsSign = createSignboard({
    width: 10,
    height: 5,
    fontSize: 16,
    align: 'center',
    canvasWidth: 400,
    canvasHeight: 200,
    textColor: '#888',
    text: "These portals go to worlds that aren't part of the main game.\n\nSome of them were created early during development and may be janky or unfinished.\n\nMost have no objective, but feel free to explore them",
  });
  lowerPortalsSign.position.set(-47.2, -33.6, 19);
  lowerPortalsSign.rotation.set(0, Math.PI / 2, 0);
  viz.scene.add(lowerPortalsSign);

  const invisibleStairSlants = loadedWorld.getObjectByName('invisible_stair_slants') as THREE.Mesh;
  invisibleStairSlants.removeFromParent();
  viz.collisionWorldLoadedCbs.push(fpCtx => fpCtx.addTriMesh(invisibleStairSlants));

  const pillars = loadedWorld.getObjectByName('pillars') as THREE.Mesh;
  pillars.material = portalFrameMat;

  const totemMat = buildCustomShader(
    {
      color: 0xcccccc,
      map: platformDiffuse,
      uvTransform: new THREE.Matrix3().scale(0.24073, 0.24073),
      normalMap: platformNormal,
      normalScale: 1,
      metalness: 1,
    },
    {
      colorShader: PlatformColorShader,
      roughnessShader: PlatformRoughnessShader,
    },
    { useTriplanarMapping: true }
  );

  const totem0 = loadedWorld.getObjectByName('totem') as THREE.Mesh;
  const totem1 = loadedWorld.getObjectByName('totem001') as THREE.Mesh;
  totem0.material = totemMat;
  totem1.material = totemMat;

  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    addMiddlePasses: (composer, viz, quality) => {
      const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
        fogMinY: -90,
        fogMaxY: -40,
        fogColorHighDensity: new THREE.Vector3(0.034, 0.024, 0.03).addScalar(0.014),
        fogColorLowDensity: new THREE.Vector3(0.08, 0.08, 0.1).addScalar(0.014),
        ambientLightColor: new THREE.Color(0x6d4444),
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
        fogDensityMultiplier: 0.32,
        postDensityMultiplier: 1.7,
        noiseMovementPerSecond: new THREE.Vector2(4.1, 4.1),
        globalScale: 1,
        halfRes: true,
        ...{
          [GraphicsQuality.Low]: { baseRaymarchStepCount: 20 },
          [GraphicsQuality.Medium]: { baseRaymarchStepCount: 30 },
          [GraphicsQuality.High]: { baseRaymarchStepCount: 60 },
        }[quality],
      });
      composer.addPass(volumetricPass);
      viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));

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
      // \/ this breaks rendering and makes the background black if enabled
      n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Medium;
      n8aoPass.setQualityMode(
        {
          [GraphicsQuality.Low]: 'Performance',
          [GraphicsQuality.Medium]: 'Low',
          [GraphicsQuality.High]: 'High',
        }[vizConf.graphics.quality]
      );

      const bloomEffect = new BloomEffect({
        intensity: 4,
        mipmapBlur: true,
        luminanceThreshold: 0.53,
        blendFunction: BlendFunction.ADD,
        luminanceSmoothing: 0.05,
        radius: 0.186,
      });
      const bloomPass = new EffectPass(viz.camera, bloomEffect);
      bloomPass.dithering = false;

      composer.addPass(bloomPass);
    },
    extraParams: {
      toneMappingExposure: 1.3,
    },
    postEffects: (() => {
      const toneMappingEffect = new ToneMappingEffect({
        mode: ToneMappingMode.LINEAR,
      });

      // return [];
      return [toneMappingEffect];
    })(),
    autoUpdateShadowMap: true,
  });

  const locations = {
    spawn: {
      pos: [-66.184, 2.928, -0.201] as [number, number, number],
      rot: [0, Math.PI / 2, 0] as [number, number, number],
    },
  };

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderSize: { height: 2.2, radius: 1.14 },
      jumpVelocity: 12,
      oobYThreshold: -80,
      dashConfig: {
        enable: true,
        useExternalVelocity: true,
        sfx: { play: true, name: 'dash' },
        chargeConfig: { curCharges: rwritable(Infinity) },
      },
      externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
      externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
    },
    debugPos: true,
    locations,
    customControlsEntries: [
      {
        key: 'f',
        action: () => viz.fpCtx?.teleportPlayer(locations.spawn.pos, locations.spawn.rot),
        label: 'Respawn',
      },
    ],
    legacyLights: false,
    sfx: {
      neededSfx: ['dash'],
    },
  };
};
