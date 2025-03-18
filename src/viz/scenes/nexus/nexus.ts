import * as THREE from 'three';
import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { N8AOPostPass } from 'n8ao';
import { ToneMappingEffect, ToneMappingMode } from 'postprocessing';
import { generateNormalMapFromTexture, loadNamedTextures, loadTexture } from 'src/viz/textureLoading';
import { buildCustomShader } from 'src/viz/shaders/customShader';
import { DashToken, initDashTokenGraphics } from '../../parkour/DashToken';
import { buildGoldMaterial, buildGreenMosaic2Material } from '../../parkour/regions/pylons/materials';
import { goto } from '$app/navigation';

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

  const towerPlinthPedestalTextureP = loadTexture(
    loader,
    'https://pub-80300747d44d418ca912329092f69f65.r2.dev/img-samples/000005.1476533049.png'
  );
  const towerPlinthPedestalTextureCombinedDiffuseNormalTextureP = towerPlinthPedestalTextureP.then(
    towerPlinthPedestalTexture => generateNormalMapFromTexture(towerPlinthPedestalTexture, {}, true)
  );

  const [
    { platformDiffuse, platformNormal },
    bgTexture,
    towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
    greenMosaic2Material,
    goldMaterial,
  ] = await Promise.all([
    loadNamedTextures(loader, {
      platformDiffuse: 'https://i.ameo.link/cce.avif',
      platformNormal: 'https://i.ameo.link/ccf.avif',
    }),
    bgTextureP,
    towerPlinthPedestalTextureCombinedDiffuseNormalTextureP,
    buildGreenMosaic2Material(loader, { ambientLightScale: 0.1 }),
    buildGoldMaterial(loader, { ambientLightScale: 0.3 }),
  ]);

  const plinthMaterial = buildCustomShader(
    {
      color: new THREE.Color(0x292929),
      metalness: 0.18,
      roughness: 0.82,
      map: towerPlinthPedestalTextureCombinedDiffuseNormalTexture,
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

  return { platformDiffuse, platformNormal, bgTexture, plinthMaterial, greenMosaic2Material, goldMaterial };
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

  const { platformDiffuse, platformNormal, bgTexture, plinthMaterial, greenMosaic2Material, goldMaterial } =
    await loadTextures();
  viz.scene.background = bgTexture;

  const mat = buildCustomShader(
    {
      color: 0x474a4d,
      map: platformDiffuse,
      roughness: 0.9,
      metalness: 0.5,
      uvTransform: new THREE.Matrix3().scale(138.073, 138.073),
      normalMap: platformNormal,
      normalScale: 1,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
      ambientLightScale: 1.8,
    },
    {},
    { useTriplanarMapping: false, tileBreaking: { type: 'neyret', patchScale: 2 } }
  );

  const platform = loadedWorld.getObjectByName('platform') as THREE.Mesh;
  platform.material = mat;

  const jumps = loadedWorld.getObjectByName('jumps') as THREE.Mesh;
  jumps.material = plinthMaterial;

  const dashToken = initDashTokenGraphics(loadedWorld, greenMosaic2Material, goldMaterial);
  viz.collisionWorldLoadedCbs.push(fpCtx => {
    const core = (dashToken.getObjectByName('dash_token_core')! as THREE.Mesh).clone();
    core.position.copy(dashToken.position);

    fpCtx.addPlayerRegionContactCb({ type: 'mesh', mesh: core }, () =>
      goto(`/pk_pylons${window.location.origin.includes('localhost') ? '' : '.html'}`)
    );
  });
  const wrappedDashToken = new DashToken(viz, dashToken);
  wrappedDashToken.userData.nocollide = true;
  wrappedDashToken.position.copy(dashToken.position);
  viz.scene.add(wrappedDashToken);

  const invisibleStairSlants = loadedWorld.getObjectByName('invisible_stair_slants') as THREE.Mesh;
  invisibleStairSlants.removeFromParent();
  viz.collisionWorldLoadedCbs.push(fpCtx => fpCtx.addTriMesh(invisibleStairSlants));

  // TODO: temp; use different mat for pillars
  const pillars = loadedWorld.getObjectByName('pillars') as THREE.Mesh;
  pillars.material = mat;

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
      return [];
    })()
  );

  return {
    spawnLocation: 'spawn',
    gravity: 30,
    player: {
      moveSpeed: { onGround: 10, inAir: 13 },
      colliderSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -80,
      dashConfig: {
        enable: true,
      },
    },
    debugPos: true,
    locations: {
      spawn: { pos: [50, 6, 0], rot: [0.1, 26, 0] },
    },
    legacyLights: false,
  };
};
