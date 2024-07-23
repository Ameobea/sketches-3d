import * as THREE from 'three';
import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import type { SceneConfig } from '..';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';
import { N8AOPostPass } from 'n8ao';
import { ToneMappingEffect, ToneMappingMode } from 'postprocessing';
import { loadNamedTextures } from 'src/viz/textureLoading';
import { buildCustomShader } from 'src/viz/shaders/customShader';

const loadTextures = async () => {
  const loader = new THREE.ImageBitmapLoader();

  const { platformDiffuse, platformNormal } = await loadNamedTextures(loader, {
    platformDiffuse: 'https://i.ameo.link/cce.avif',
    platformNormal: 'https://i.ameo.link/ccf.avif',
  });

  return { platformDiffuse, platformNormal };
};

export const processLoadedScene = async (
  viz: VizState,
  loadedWorld: THREE.Group,
  vizConf: VizConfig
): Promise<SceneConfig> => {
  const ambientLight = new THREE.AmbientLight(0xffffff, 2.4);
  viz.scene.add(ambientLight);

  const dirLight = new THREE.DirectionalLight(0xdde6f1, 3.6);
  dirLight.position.set(142 * 1.1, 303 * 1.1, 380 * 1.1);
  dirLight.target.position.set(0, 0, 0);
  dirLight.castShadow = false;

  viz.scene.add(dirLight);

  const { platformDiffuse, platformNormal } = await loadTextures();

  const mat = buildCustomShader(
    {
      color: 0x474a50,
      map: platformDiffuse,
      roughness: 0.9,
      metalness: 0.4,
      uvTransform: new THREE.Matrix3().scale(138.073, 138.073),
      normalMap: platformNormal,
      normalScale: 0.95,
      normalMapType: THREE.TangentSpaceNormalMap,
      mapDisableDistance: null,
      ambientLightScale: 1.8,
    },
    {},
    { useTriplanarMapping: false, tileBreaking: { type: 'neyret', patchScale: 2 } }
  );

  const platform = loadedWorld.getObjectByName('platform') as THREE.Mesh;
  platform.material = mat;

  // TODO: temp; use different mat for pillars
  const pillars = loadedWorld.getObjectByName('pillars') as THREE.Mesh;
  pillars.material = mat;

  // TODO: temp; use different mat for totems
  const totem0 = loadedWorld.getObjectByName('totem') as THREE.Mesh;
  const totem1 = loadedWorld.getObjectByName('totem001') as THREE.Mesh;
  totem0.material = mat;
  totem1.material = mat;

  configureDefaultPostprocessingPipeline(
    viz,
    vizConf.graphics.quality,
    (composer, viz, quality) => {
      const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
        fogMinY: -60,
        fogMaxY: -40,
        fogColorHighDensity: new THREE.Vector3(0.034, 0.024, 0.03),
        fogColorLowDensity: new THREE.Vector3(0.08, 0.08, 0.1),
        ambientLightColor: new THREE.Color(0x4d2424),
        ambientLightIntensity: 1.2,
        heightFogStartY: -70,
        heightFogEndY: -55,
        heightFogFactor: 0.14,
        maxRayLength: 1000,
        minStepLength: 0.1,
        noiseBias: 0.7,
        noisePow: 2.4,
        fogFadeOutRangeY: 8,
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
      n8aoPass.configuration.halfRes = vizConf.graphics.quality <= GraphicsQuality.Low;
      n8aoPass.setQualityMode(
        {
          [GraphicsQuality.Low]: 'Low',
          [GraphicsQuality.Medium]: 'Low',
          [GraphicsQuality.High]: 'Medium',
        }[vizConf.graphics.quality]
      );
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
      colliderCapsuleSize: { height: 2.2, radius: 0.8 },
      jumpVelocity: 12,
      oobYThreshold: -10,
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
