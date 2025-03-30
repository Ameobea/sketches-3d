import * as THREE from 'three';
import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { N8AOPostPass } from 'n8ao';
import { ToneMappingEffect, ToneMappingMode } from 'postprocessing';
import { generateNormalMapFromTexture, loadTexture } from 'src/viz/textureLoading';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { DashToken, initDashTokenGraphics } from '../../parkour/DashToken';
import { buildGoldMaterial, buildGreenMosaic2Material } from '../../parkour/regions/pylons/materials';
import { goto } from '$app/navigation';
import type { BulletPhysics } from 'src/viz/collision';
import {
  buildGrayFossilRockMaterial,
  GrayFossilRockTextures,
} from 'src/viz/materials/GrayFossilRock/GrayFossilRockMaterial';
import BridgeMistColorShader from 'src/viz/shaders/bridge2/bridge_top_mist/color.frag?raw';
import { createSignboard, type CreateSignboardArgs } from 'src/viz/helpers/signboardBuilder';
import { mix, smoothstep } from 'src/viz/util/util';

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

  return { platformMat, bgTexture, lazyMatsP, platformDiffuse, platformNormal };
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
  const pointLightColor = new THREE.Color(0x9d4444);
  const pointLight = new THREE.PointLight(pointLightColor, 1, 0, 0);
  pointLight.castShadow = false;
  pointLight.position.copy(pointLightPos);
  viz.scene.add(pointLight);

  viz.registerBeforeRenderCb(() => {
    const pointLightActivation = 1 - smoothstep(-20, 0, viz.camera.position.y);
    pointLight.intensity = 17 * pointLightActivation;
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

  // TODO: Temp?
  const checkpointMat = buildCustomShader(
    { metalness: 0, alphaTest: 0.05, transparent: true },
    { colorShader: BridgeMistColorShader },
    { disableToneMapping: true }
  );
  viz.registerBeforeRenderCb(curTimeSeconds => checkpointMat.setCurTimeSeconds(curTimeSeconds));

  for (const portal of portals) {
    portal.userData.nocollide = true;
    portal.material = checkpointMat;
    portal.userData.noLight = true;

    if (!portal.name.includes('_')) {
      portal.visible = false;
    }
  }

  const { platformMat, bgTexture, lazyMatsP, platformDiffuse, platformNormal } = await loadTextures();
  viz.scene.background = bgTexture;

  const platform = loadedWorld.getObjectByName('platform') as THREE.Mesh;
  platform.material = platformMat;

  // TODO: Temp
  const portalFrameMat = buildCustomShader(
    {
      color: 0x080808,
      uvTransform: new THREE.Matrix3().scale(0.24073, 0.24073),
      normalMap: platformNormal,
      normalScale: 0.95,
      normalMapType: THREE.TangentSpaceNormalMap,
    },
    {},
    { useGeneratedUVs: true, randomizeUVOffset: true }
  );

  const addPortalFrameSign = (portalFrame: THREE.Mesh, params: CreateSignboardArgs) => {
    const sign = createSignboard({
      width: 7.75,
      height: 7 / 2,
      fontSize: 56,
      align: 'center',
      canvasWidth: 400,
      canvasHeight: 200,
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

  // TODO: temp; use different mat for pillars
  const pillars = loadedWorld.getObjectByName('pillars') as THREE.Mesh;
  pillars.material = portalFrameMat;

  // TODO: temp; use different mat for totems
  const totemMat = buildCustomShader(
    {
      color: 0x474a50,
      map: platformDiffuse,
      roughness: 0.9,
      metalness: 0.5,
      uvTransform: new THREE.Matrix3().scale(0.4073, 0.4073),
      normalMap: platformNormal,
      normalScale: 0.95,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
      ambientLightScale: 1.8,
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
    },
    undefined,
    { toneMappingExposure: 1.48 },
    (() => {
      const toneMappingEffect = new ToneMappingEffect({
        mode: ToneMappingMode.LINEAR,
      });

      return [toneMappingEffect];
    })(),
    true
  );

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
      },
      externalVelocityAirDampingFactor: new THREE.Vector3(0.32, 0.3, 0.32),
      externalVelocityGroundDampingFactor: new THREE.Vector3(0.9992, 0.9992, 0.9992),
    },
    debugPos: true,
    locations: {
      spawn: {
        pos: [49.83, 7.062, 0],
        rot: [0, 1.5, 0],
      },
    },
    legacyLights: false,
    sfx: {
      neededSfx: ['dash'],
    },
  };
};
