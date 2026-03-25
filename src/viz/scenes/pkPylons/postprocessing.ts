import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';

export const initPylonsPostprocessing = (viz: Viz, vizConf: VizConfig, autoUpdateShadowMap = false) =>
  configureDefaultPostprocessingPipeline({
    viz,
    quality: vizConf.graphics.quality,
    addMiddlePasses: (composer, viz, quality) => {
      const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
        fogMinY: -140,
        fogMaxY: -5,
        fogColorHighDensity: new THREE.Vector3(0.12, 0.15, 0.18).multiplyScalar(0.7),
        fogColorLowDensity: new THREE.Vector3(0.8, 0.8, 0.8),
        ambientLightColor: new THREE.Color(0xffffff),
        ambientLightIntensity: 1.2,
        heightFogStartY: -140,
        heightFogEndY: -125,
        heightFogFactor: 0.14,
        maxRayLength: 1000,
        minStepLength: 0.1,
        noiseBias: 0.1,
        noisePow: 3.1,
        fogFadeOutRangeY: 32,
        fogFadeOutPow: 0.6,
        fogDensityMultiplier: 0.22,
        postDensityMultiplier: 1.4,
        noiseMovementPerSecond: new THREE.Vector2(4.1, 4.1),
        globalScale: 1,
        halfRes: true,
        ...{
          [GraphicsQuality.Low]: {
            baseRaymarchStepCount: 40,
            octaveCount: 3,
            renderScale: 0.25,
            fogFadeOutRangeY: 4,
            fogFadeOutPow: 1.6,
            fogDensityMultiplier: 0.32,
            globalScale: 1.4,
            jbuExtent: 1,
            jbuSpatialSigma: 1.3,
            jbuDepthSigma: 0.05,
            fogColorHighDensity: new THREE.Vector3(0.3, 0.35, 0.44),
          },
          [GraphicsQuality.Medium]: { baseRaymarchStepCount: 130 },
          [GraphicsQuality.High]: { baseRaymarchStepCount: 240 },
        }[quality],
      });
      composer.addPass(volumetricPass);
      viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
    },
    toneMapping: { mode: 'neutral', exposure: 0.85 },
    autoUpdateShadowMap,
    emissiveBypass: true,
    emissiveBloom:
      vizConf.graphics.quality > GraphicsQuality.Low
        ? { intensity: 2.5, levels: 2, luminanceThreshold: 0.08, radius: 0.1 }
        : null,
  });
