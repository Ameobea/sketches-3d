import { EffectComposer, EffectPass, SMAAEffect, SMAAPreset } from 'postprocessing';
import * as THREE from 'three';

import type { Viz } from 'src/viz';
import { GraphicsQuality } from 'src/viz/conf';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';

export const configurePostprocessing = (viz: Viz, quality: GraphicsQuality) => {
  const effectComposer = new EffectComposer(viz.renderer, {
    multisampling: 0,
    frameBufferType: THREE.HalfFloatType,
  });

  viz.renderer.autoClear = false;
  viz.renderer.autoClearColor = true;
  viz.renderer.autoClearDepth = false;
  const depthPrePassMaterial = new THREE.MeshBasicMaterial();
  const depthPass = new DepthPass(viz.scene, viz.camera, depthPrePassMaterial, true);
  depthPass.skipShadowMapUpdate = true;
  effectComposer.addPass(depthPass);

  const renderPass = new MainRenderPass(viz.scene, viz.camera);
  renderPass.skipShadowMapUpdate = true;
  renderPass.needsDepthTexture = true;
  effectComposer.addPass(renderPass);

  const volumetricPass = new VolumetricPass(viz.scene, viz.camera, {
    fogMinY: -50,
    fogMaxY: 56,
    fogColorLowDensity: new THREE.Vector3(0.23, 0.27, 0.27),
    fogColorHighDensity: new THREE.Vector3(1, 1, 1),
    ambientLightColor: new THREE.Color(0xffffff),
    ambientLightIntensity: 1.2,
    heightFogStartY: -8,
    heightFogEndY: 14,
    heightFogFactor: 0.34,
    maxRayLength: 300,
    minStepLength: 0.1,
    noiseBias: 0.5,
    noisePow: 3.6,
    halfRes: true,
    baseRaymarchStepCount: 110,
    maxRaymarchStepCount: 2000,
    fogFadeOutRangeY: 2.8,
    fogFadeOutPow: 1,
    postDensityPow: 1,
    postDensityMultiplier: 0.97,
    maxDensity: 1,
    noiseMovementPerSecond: new THREE.Vector2(2.8, 0.8),
    // fogDensityMultiplier: 0.146,
  });
  viz.registerBeforeRenderCb(curTimeSeconds => volumetricPass.setCurTimeSeconds(curTimeSeconds));
  effectComposer.addPass(volumetricPass);

  const smaaEffect = new SMAAEffect({
    preset: {
      [GraphicsQuality.Low]: SMAAPreset.LOW,
      [GraphicsQuality.Medium]: SMAAPreset.MEDIUM,
      [GraphicsQuality.High]: SMAAPreset.HIGH,
    }[quality],
  });
  const fxPass = new EffectPass(viz.camera, smaaEffect);
  effectComposer.addPass(fxPass);

  viz.renderer.toneMapping = THREE.ACESFilmicToneMapping;
  // viz.renderer.toneMappingExposure = 1.8;

  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
    viz.renderer.shadowMap.autoUpdate = false;
    viz.renderer.shadowMap.needsUpdate = false;
  });
};
