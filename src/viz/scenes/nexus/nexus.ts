import * as THREE from 'three';
import { goto } from '$app/navigation';
import { N8AOPostPass } from 'n8ao';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { BlendFunction, BloomEffect, EffectPass, ToneMappingEffect, ToneMappingMode } from 'postprocessing';
import { generateNormalMapFromTexture, loadTexture } from 'src/viz/textureLoading';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { DashToken, initDashTokenGraphics } from '../../parkour/DashToken';
import { buildGoldMaterial, buildGreenMosaic2Material } from '../../parkour/regions/pylons/materials';
import type { BulletPhysics } from 'src/viz/collision';
import {
  buildGrayFossilRockMaterial,
  GrayFossilRockTextures,
} from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';
import { createSignboard, type CreateSignboardArgs } from 'src/viz/helpers/signboardBuilder';
import { mix, smoothstep } from 'src/viz/util/util';
import { rwritable } from 'src/viz/util/TransparentWritable';
import { buildCheckpointMaterial } from 'src/viz/materials/Checkpoint/CheckpointMaterial';
import { buildGrayStoneBricksFloorMaterial } from 'src/viz/materials/GrayStoneBricksFloor/GrayStoneBricksFloorMaterial';

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

  const towerPlinthPedestalTextureP = loadTexture(loader, 'https://i.ameo.link/cwa.avif');
  const towerPlinthPedestalTextureCombinedDiffuseNormalTextureP = towerPlinthPedestalTextureP.then(
    towerPlinthPedestalTexture => generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
  );

  const lazyMatsP = Promise.all([
    buildGreenMosaic2Material(loader, { ambientLightScale: 0.1 }),
    buildGoldMaterial(loader, { ambientLightScale: 0.3 }),
  ]).then(async ([greenMosaic2Material, goldMaterial]) => {
    const plinthMaterial = buildCustomShader(
      {
        color: new THREE.Color(0x292929),
        metalness: 0.18,
        roughness: 0.82,
        map: await towerPlinthPedestalTextureCombinedDiffuseNormalTextureP,
        uvTransform: new THREE.Matrix3().scale(0.8, 0.8),
        mapDisableDistance: null,
        normalScale: 5.2,
        ambientLightScale: 1.8,
      },
      {},
      {
        usePackedDiffuseNormalGBA: true,
        useGeneratedUVs: true,
        randomizeUVOffset: true,
        tileBreaking: { type: 'neyret', patchScale: 0.9 },
      }
    );

    return { plinthMaterial, greenMosaic2Material, goldMaterial };
  });

  const [platformMat, bgTexture, { platformDiffuse, platformNormal }] = await Promise.all([
    platformMatP,
    bgTextureP,
    platformTexsP,
  ]);

  return { platformMat, bgTexture, lazyMatsP, platformDiffuse, platformNormal, loader };
};

export const processLoadedScene = async (
  viz: Viz,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
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

  const PortalColorByName: Record<string, [number, number, number]> = {
    tutorial: [0.4, 0.7, 0.4],
    stone: [0.11, 0.31, 0.7],
  };

  for (const portal of portals) {
    portal.userData.nocollide = true;
    portal.material = buildCheckpointMaterial(viz, PortalColorByName[portal.name.split('_')[1]]);
    portal.userData.noLight = true;

    if (!portal.name.includes('_')) {
      portal.visible = false;
    }
  }

  const { platformMat, bgTexture, lazyMatsP, platformDiffuse, platformNormal, loader } = await loadTextures();
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
      colorShader: `
const vec3[5] PLATFORM_COLOR_RAMP = vec3[5](vec3(0.0712, 0.091, 0.0904), vec3(0.0912, 0.131, 0.1304), vec3(0.22, 0.21, 0.27), vec3(0.52, 0.54, 0.73), vec3(0.22, 0.24, 0.23));

vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float brightness = fract(baseColor.r * 1.5);
  float rampIndex = brightness * float(5 - 1);
  int low = int(floor(rampIndex));
  int high = int(ceil(rampIndex));
  float t = fract(rampIndex);
  vec3 rampColor = mix(PLATFORM_COLOR_RAMP[low], PLATFORM_COLOR_RAMP[high], t);
  vec3 outColor = mix(rampColor, baseColor, 0.2);
  return vec4(outColor * 0.3, 1.);
}
`,
      roughnessShader: `
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(ctx.diffuseColor.b * 24.5, 2.5) * 0.2;
  shinyness = clamp(shinyness, 0.0, 0.6);
  return 1. - shinyness;
}`,
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
    { useTriplanarMapping: false, tileBreaking: { type: 'neyret', patchScale: 2 } }
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
    { useTriplanarMapping: false, tileBreaking: { type: 'neyret', patchScale: 2 } }
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
      width: 7.75,
      height: 7 / 2,
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
    sign.position.y += 12.3;
    // move the sign forward wrt. the direction it's facing a bit
    sign.position.addScaledVector(portalFrame.getWorldDirection(new THREE.Vector3()), -2.3);
    viz.scene.add(sign);
  };

  for (const portalFrame of portalFrames) {
    portalFrame.material = portalFrameMat;

    if (portalFrame.name.includes('tutorial')) {
      addPortalFrameSign(portalFrame, { text: 'Tutorial' });
    } else if (portalFrame.name.includes('pylons')) {
      addPortalFrameSign(portalFrame, { text: 'Pylons' });
    } else if (portalFrame.name.includes('movementv2')) {
      addPortalFrameSign(portalFrame, { text: 'Movement V2' });
    } else if (portalFrame.name.includes('plats')) {
      addPortalFrameSign(portalFrame, { text: 'Plats' });
    } else if (portalFrame.name.includes('cornered')) {
      addPortalFrameSign(portalFrame, { text: 'Cornered' });
    } else if (portalFrame.name.includes('stone')) {
      addPortalFrameSign(portalFrame, { text: 'Stone' });
    } else if (portalFrame.name.includes('basalt')) {
      addPortalFrameSign(portalFrame, { text: 'Basalt' });
    }
  }

  viz.collisionWorldLoadedCbs.push(fpCtx => {
    for (const portal of portals) {
      if (portal.name.includes('_tutorial')) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          goto(`/tutorial${window.location.origin.includes('localhost') ? '' : '.html'}`);
        });
      } else if (portal.name.includes('_pylons')) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          goto(`/pk_pylons${window.location.origin.includes('localhost') ? '' : '.html'}`);
        });
      } else if (portal.name.includes('_movementv2')) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          goto(`/movement_v2${window.location.origin.includes('localhost') ? '' : '.html'}`);
        });
      } else if (portal.name.includes('_plats')) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          goto(`/plats${window.location.origin.includes('localhost') ? '' : '.html'}`);
        });
      } else if (portal.name.includes('_cornered')) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          goto(`/cornered${window.location.origin.includes('localhost') ? '' : '.html'}`);
        });
      } else if (portal.name.includes('_stone')) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          goto(`/stone${window.location.origin.includes('localhost') ? '' : '.html'}`);
        });
      } else if (portal.name.includes('_basalt')) {
        fpCtx.addPlayerRegionContactCb({ type: 'convexHull', mesh: portal }, () => {
          goto(`/basalt${window.location.origin.includes('localhost') ? '' : '.html'}`);
        });
      } else {
        portal.visible = false;
      }
    }
  });

  const jumps = loadedWorld.getObjectByName('jumps') as THREE.Mesh;
  jumps.visible = false;
  (loadedWorld.getObjectByName('dash_token')! as THREE.Mesh).userData.nocollide = true;

  lazyMatsP.then(({ plinthMaterial, greenMosaic2Material, goldMaterial }) => {
    jumps.material = plinthMaterial;
    jumps.visible = true;

    const dashToken = initDashTokenGraphics(loadedWorld, greenMosaic2Material, goldMaterial);
    const cb = (fpCtx: BulletPhysics) => {
      const core = (dashToken.getObjectByName('dash_token_core')! as THREE.Mesh).clone();
      core.position.copy(dashToken.position);

      fpCtx.addPlayerRegionContactCb({ type: 'mesh', mesh: core }, () =>
        goto(`/pk_pylons${window.location.origin.includes('localhost') ? '' : '.html'}`)
      );
    };
    if (viz.fpCtx) {
      cb(viz.fpCtx);
    } else {
      viz.collisionWorldLoadedCbs.push(cb);
    }

    const wrappedDashToken = new DashToken(viz, dashToken);
    wrappedDashToken.userData.nocollide = true;
    wrappedDashToken.position.copy(dashToken.position);
    viz.scene.add(wrappedDashToken);
  });

  const invisibleStairSlants = loadedWorld.getObjectByName('invisible_stair_slants') as THREE.Mesh;
  invisibleStairSlants.removeFromParent();
  viz.collisionWorldLoadedCbs.push(fpCtx => fpCtx.addTriMesh(invisibleStairSlants));

  const pillars = loadedWorld.getObjectByName('pillars') as THREE.Mesh;
  pillars.material = portalFrameMat;

  const totemMat = buildCustomShader(
    {
      color: 0x1c0a0a,
      uvTransform: new THREE.Matrix3().scale(0.24073, 0.24073),
      normalMap: platformNormal,
      normalScale: 0.65,
      normalMapType: THREE.TangentSpaceNormalMap,
      roughness: 0.8,
      metalness: 0,
    },
    {},
    { useTriplanarMapping: true }
  );

  const totem0 = loadedWorld.getObjectByName('totem') as THREE.Mesh;
  const totem1 = loadedWorld.getObjectByName('totem001') as THREE.Mesh;
  totem0.material = totemMat;
  totem1.material = totemMat;

  configureDefaultPostprocessingPipeline(
    viz,
    vizConf.graphics.quality,
    (composer, viz, quality) => {
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
        compositor: { edgeRadius: 4, edgeStrength: 2 },
        ...{
          [GraphicsQuality.Low]: { baseRaymarchStepCount: 20 },
          [GraphicsQuality.Medium]: { baseRaymarchStepCount: 40 },
          [GraphicsQuality.High]: { baseRaymarchStepCount: 80 },
        }[quality],
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
        n8aoPass.configuration.intensity = 2;
        n8aoPass.configuration.aoRadius = 5;
        // \/ this breaks rendering and makes the background black if enabled
        // n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Low;
        n8aoPass.setQualityMode(
          {
            [GraphicsQuality.Low]: 'Performance',
            [GraphicsQuality.Medium]: 'Low',
            [GraphicsQuality.High]: 'High',
          }[vizConf.graphics.quality]
        );
      }

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
    undefined,
    {
      toneMappingExposure: 1.3,
    },
    (() => {
      const toneMappingEffect = new ToneMappingEffect({
        mode: ToneMappingMode.LINEAR,
      });

      // return [];
      return [toneMappingEffect];
    })(),
    true
  );

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
