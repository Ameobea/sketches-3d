import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality, type VizConfig } from 'src/viz/conf';
import { configureDefaultPostprocessingPipeline } from 'src/viz/postprocessing/defaultPostprocessing';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';

export const initPylonsPostprocessing = (viz: VizState, vizConf: VizConfig) => {
  configureDefaultPostprocessingPipeline(viz, vizConf.graphics.quality, (composer, viz, quality) => {
    const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
      fogMinY: -140,
      fogMaxY: -5,
      fogColorHighDensity: new THREE.Vector3(0.32, 0.35, 0.38),
      fogColorLowDensity: new THREE.Vector3(0.9, 0.9, 0.9),
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
      compositor: { edgeRadius: 4, edgeStrength: 2 },
      ...{
        [GraphicsQuality.Low]: { baseRaymarchStepCount: 88 },
        [GraphicsQuality.Medium]: { baseRaymarchStepCount: 130 },
        [GraphicsQuality.High]: { baseRaymarchStepCount: 240 },
      }[quality],
    });
    composer.addPass(volumetricPass);
    viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
  });
};
