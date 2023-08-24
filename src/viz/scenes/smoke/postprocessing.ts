import { N8AOPostPass } from 'n8ao';
import { EffectComposer, EffectPass, KernelSize, RenderPass, SMAAEffect, SMAAPreset } from 'postprocessing';
import * as THREE from 'three';
import { GodraysPass, type GodraysPassParams } from 'three-good-godrays';

import type { VizState } from 'src/viz';
import { smoothstep } from 'src/viz/util';

/**
 * We want to back off the AO when outside of the building
 */
const computeN8AOIntensity = (playerPos: THREE.Vector3): number => {
  if (playerPos.z < 12 && playerPos.x < 5) {
    return 7;
  }
  if (playerPos.z > 50) {
    return 0;
  }

  const shutoffStartX = -25;
  const shutoffEndX = -5;
  const factor = 1 - smoothstep(shutoffStartX, shutoffEndX, playerPos.x);
  return 0 + 7 * factor;
};

export const configurePostprocessing = (viz: VizState, dirLight: THREE.DirectionalLight) => {
  const effectComposer = new EffectComposer(viz.renderer, { multisampling: 2 });
  const renderPass = new RenderPass(viz.scene, viz.camera);
  effectComposer.addPass(renderPass);

  const godraysParams: GodraysPassParams = {
    color: new THREE.Color().copy(dirLight.color),
    edgeRadius: 1,
    edgeStrength: 1,
    distanceAttenuation: 1,
    density: 1 / 8,
    maxDensity: 1,
    raymarchSteps: 80,
    blur: { kernelSize: KernelSize.VERY_LARGE, variance: 0.45 },
  };

  const n8aoPass = new N8AOPostPass(
    viz.scene,
    viz.camera,
    viz.renderer.domElement.width,
    viz.renderer.domElement.height
  );
  n8aoPass.gammaCorrection = false;
  n8aoPass.configuration.intensity = 7;
  n8aoPass.configuration.aoRadius = 9;
  // n8aoPass.configuration.distanceFalloff = 0.5;
  // n8aoPass.configuration.halfRes = true;
  n8aoPass.setQualityMode('Medium');
  // effectComposer.addPass(n8aoPass);

  const godraysEffect = new GodraysPass(dirLight, viz.camera, godraysParams);
  effectComposer.addPass(godraysEffect);

  const smaaEffect2 = new SMAAEffect({ preset: SMAAPreset.MEDIUM });
  const smaaPass2 = new EffectPass(viz.camera, smaaEffect2);
  effectComposer.addPass(smaaPass2);

  let lastN8AOIntensity = Infinity;
  let n8aoPassEnabled = false;
  viz.setRenderOverride(timeDiffSeconds => {
    const newN8AOIntensity = computeN8AOIntensity(viz.camera.position);
    if (newN8AOIntensity !== lastN8AOIntensity) {
      n8aoPass.configuration.intensity = newN8AOIntensity;
      lastN8AOIntensity = newN8AOIntensity;

      if (newN8AOIntensity > 0 && !n8aoPassEnabled) {
        effectComposer.addPass(n8aoPass, 1);
        n8aoPassEnabled = true;
      } else if (newN8AOIntensity === 0 && n8aoPassEnabled) {
        effectComposer.removePass(n8aoPass);
        n8aoPassEnabled = false;
      }
    }

    effectComposer.render(timeDiffSeconds);
    viz.renderer.shadowMap.autoUpdate = false;
  });

  viz.renderer.toneMapping = THREE.CineonToneMapping;
  viz.renderer.toneMappingExposure = 1.8;
};
