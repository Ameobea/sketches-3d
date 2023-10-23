import { EffectComposer, EffectPass, RenderPass, SMAAEffect, SMAAPreset } from 'postprocessing';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality } from 'src/viz/conf';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';

const populateShadowMap = (viz: VizState) => {
  const shadows: THREE.DirectionalLightShadow[] = [];
  viz.scene.traverse(obj => {
    if (obj instanceof THREE.DirectionalLight) {
      shadows.push(obj.shadow);
    }
  });

  // Render the scene once to populate the shadow map
  shadows.forEach(shadow => {
    shadow.needsUpdate = true;
  });
  viz.renderer.shadowMap.needsUpdate = true;
  viz.renderer.render(viz.scene, viz.camera);
  shadows.forEach(shadow => {
    shadow.needsUpdate = false;
    shadow.autoUpdate = false;
  });
  viz.renderer.shadowMap.needsUpdate = false;
  viz.renderer.shadowMap.autoUpdate = false;
  viz.renderer.shadowMap.enabled = true;
};

interface ExtraPostprocessingParams {
  toneMappingExposure?: number;
}

export const configureDefaultPostprocessingPipeline = (
  viz: VizState,
  quality: GraphicsQuality,
  addMiddlePasses?: (composer: EffectComposer, viz: VizState, quality: GraphicsQuality) => void,
  onFirstRender?: () => void,
  extraParams: Partial<ExtraPostprocessingParams> = {}
) => {
  const effectComposer = new EffectComposer(viz.renderer, { multisampling: 0 });

  viz.renderer.autoClear = false;
  viz.renderer.autoClearColor = true;
  viz.renderer.autoClearDepth = false;
  const depthPrePassMaterial = new THREE.MeshBasicMaterial();
  const depthPass = new DepthPass(viz.scene, viz.camera, depthPrePassMaterial);
  depthPass.skipShadowMapUpdate = true;
  effectComposer.addPass(depthPass);

  const renderPass = new MainRenderPass(viz.scene, viz.camera);
  renderPass.skipShadowMapUpdate = true;
  renderPass.needsDepthTexture = true;
  effectComposer.addPass(renderPass);

  addMiddlePasses?.(effectComposer, viz, quality);

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
  if (extraParams.toneMappingExposure) {
    viz.renderer.toneMappingExposure = extraParams.toneMappingExposure;
  }
  // viz.renderer.toneMappingExposure = 1.8;

  let didRender = false;
  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
    viz.renderer.shadowMap.autoUpdate = false;
    viz.renderer.shadowMap.needsUpdate = false;

    // For some reason, the shadow map that we render at the start of everything is getting cleared at some
    // point during the setup of this postprocessing pipeline.
    //
    // So, we have to re-populate the shadowmap so that it can be used to power the godrays and, well, shadows.
    if (!didRender) {
      didRender = true;
      populateShadowMap(viz);
    }
  });
};
