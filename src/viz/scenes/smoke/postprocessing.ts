import { N8AOPostPass } from 'n8ao';
import {
  BlendFunction,
  EffectComposer,
  EffectPass,
  KernelSize,
  RenderPass,
  SelectiveBloomEffect,
  SMAAEffect,
  SMAAPreset,
} from 'postprocessing';
import * as THREE from 'three';
import { GodraysPass, type GodraysPassParams } from 'three-good-godrays';

import type { VizState } from 'src/viz';
import { smoothstep, smoothstepScale } from 'src/viz/util';

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
  if (playerPos.y < -10) {
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

  // Selective bloom on the pipe lights
  const pipeLightBloomEffect = new SelectiveBloomEffect(viz.scene, viz.camera, {
    intensity: 33,
    blendFunction: BlendFunction.LINEAR_DODGE,
    luminanceThreshold: 0,
    kernelSize: KernelSize.LARGE,
    // mipmapBlur: true,
    radius: 0.9,
    // resolutionScale: 2,
  } as any);
  pipeLightBloomEffect.inverted = false;
  pipeLightBloomEffect.ignoreBackground = true;
  const pipeLights = viz.scene
    .getObjectByName('Scene')!
    .children.filter(c => c.name === 'pipe_light' || c.name.startsWith('pipe_light0'));
  pipeLightBloomEffect.selection.set(pipeLights);

  const smaaEffect2 = new SMAAEffect({ preset: SMAAPreset.MEDIUM });
  const smaaPass2 = new EffectPass(viz.camera, pipeLightBloomEffect, smaaEffect2);
  effectComposer.addPass(smaaPass2);

  viz.renderer.toneMapping = THREE.CineonToneMapping;
  viz.renderer.toneMappingExposure = 1.8;

  const averagePipeLightPos = pipeLights
    .reduce((acc, light) => acc.add(light.position), new THREE.Vector3())
    .divideScalar(pipeLights.length);
  const black = new THREE.Color(0x0);
  const baseBGColor = new THREE.Color(0x8f4509);
  let lastDownFactor = Infinity;
  viz.registerBeforeRenderCb(curTimeSecs => {
    const distanceToPlayer = viz.camera.position.distanceTo(averagePipeLightPos);
    // when player is < 60 meters away:
    //  * intensity = 23
    //  * scale multiplier = 1
    // when player is 130 meters away:
    //  * intensity = 1
    //  * scale multiplier = 12

    const scaleMultiplier = smoothstep(60, 130, distanceToPlayer) * 11 + 1;
    const intensity = (1 - smoothstep(60, 130, distanceToPlayer)) * 22 + 1;
    pipeLightBloomEffect.intensity = intensity;

    let pulse = Math.sin(curTimeSecs * 2);
    // sharpen the pulse a bit
    pulse = Math.sign(pulse) * Math.pow(Math.abs(pulse), 0.7);
    const scale = (pulse * 0.2 + 0.4) * scaleMultiplier;
    for (const pipeLight of pipeLights) {
      pipeLight.scale.set(scale, scale, scale);
    }

    // fade eveerything to black as player descends
    const [fadeoutStartY, fadeoutEndY] = [-120, -20];
    const playerY = viz.camera.position.y;
    const downFactor = smoothstep(fadeoutStartY, fadeoutEndY, playerY);
    if (downFactor !== lastDownFactor) {
      lastDownFactor = downFactor;
      viz.renderer.toneMappingExposure = smoothstepScale(fadeoutStartY, fadeoutEndY, playerY, 0.0, 1.8);

      (viz.scene.background as THREE.Color).copy(black).lerp(baseBGColor, downFactor);
      godraysEffect.setParams({
        ...godraysParams,
        color: godraysParams.color.copy(dirLight.color).multiplyScalar(downFactor),
      });
      viz.scene.fog!.color.copy(black).lerp(baseBGColor, downFactor);
    }
  });

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
};
