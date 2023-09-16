import { EffectComposer, EffectPass, SMAAEffect, SMAAPreset } from 'postprocessing';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality } from 'src/viz/conf';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';
import { VolumetricPass } from 'src/viz/shaders/volumetric/volumetric';

export const configurePostprocessing = (viz: VizState, quality: GraphicsQuality) => {
  const effectComposer = new EffectComposer(viz.renderer, { multisampling: 0 });

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

  const volumetricPass = new VolumetricPass(viz.camera);
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
