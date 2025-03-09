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
import { GraphicsQuality } from 'src/viz/conf';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';
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

const USE_DEPTH_PREPASS = true;

export const configurePostprocessing = (
  viz: VizState,
  dirLight: THREE.DirectionalLight,
  quality: GraphicsQuality,
  onFirstRender: () => void
) => {
  const effectComposer = new EffectComposer(viz.renderer, {
    multisampling: 0,
    frameBufferType: THREE.HalfFloatType,
  });

  if (USE_DEPTH_PREPASS) {
    viz.renderer.autoClear = false;
    viz.renderer.autoClearColor = true;
    viz.renderer.autoClearDepth = false;
    const depthPass = new DepthPass(viz.scene, viz.camera, new THREE.MeshBasicMaterial(), true);
    depthPass.skipShadowMapUpdate = true;
    effectComposer.addPass(depthPass);

    const renderPass = new MainRenderPass(viz.scene, viz.camera);
    renderPass.skipShadowMapUpdate = true;
    renderPass.needsDepthTexture = true;
    effectComposer.addPass(renderPass);
  } else {
    const renderPass = new RenderPass(viz.scene, viz.camera);
    renderPass.skipShadowMapUpdate = true;
    effectComposer.addPass(renderPass);
  }

  const godraysParams: GodraysPassParams = {
    color: new THREE.Color().copy(dirLight.color),
    edgeRadius: 1,
    edgeStrength: 1,
    distanceAttenuation: 1,
    density: 1 / 8,
    maxDensity: 1,
    raymarchSteps: {
      [GraphicsQuality.Low]: 50,
      [GraphicsQuality.Medium]: 65,
      [GraphicsQuality.High]: 80,
    }[quality] as any,
    blur: {
      kernelSize: {
        [GraphicsQuality.Low]: KernelSize.MEDIUM,
        [GraphicsQuality.Medium]: KernelSize.LARGE,
        [GraphicsQuality.High]: KernelSize.VERY_LARGE,
      }[quality],
      variance: 0.45,
    },
    gammaCorrection: false,
  };

  let n8aoPass: typeof N8AOPostPass | null = null;
  if (quality > GraphicsQuality.Low) {
    n8aoPass = new N8AOPostPass(
      viz.scene,
      viz.camera,
      viz.renderer.domElement.width,
      viz.renderer.domElement.height
    );
    n8aoPass.gammaCorrection = false;
    n8aoPass.configuration.intensity = 7;
    n8aoPass.configuration.aoRadius = 9;
    // \/ this breaks rendering and makes the background black if enabled
    // n8aoPass.configuration.halfRes = quality <= GraphicsQuality.Low;
    n8aoPass.setQualityMode(
      {
        [GraphicsQuality.Low]: 'Performance',
        [GraphicsQuality.Medium]: 'Low',
        [GraphicsQuality.High]: 'Medium',
      }[quality]
    );
  }

  const godraysEffect = new GodraysPass(dirLight, viz.camera, godraysParams);
  effectComposer.addPass(godraysEffect);

  // Make the pipe lights glow through the godrays
  const pipeLightBloomEffect = new SelectiveBloomEffect(viz.scene, viz.camera, {
    intensity: 33,
    blendFunction: BlendFunction.LINEAR_DODGE,
    luminanceThreshold: 0,
    kernelSize: KernelSize.LARGE,
    radius: 0.9,
  } as any);
  pipeLightBloomEffect.inverted = false;
  pipeLightBloomEffect.ignoreBackground = true;
  const pipeAndTorchLights = viz.scene
    .getObjectByName('Scene')!
    .children.filter(
      c => c.name === 'pipe_light' || c.name.startsWith('pipe_light0') || c.name === 'torch_light'
    );
  const pipeLights = pipeAndTorchLights.filter(l => l.name.includes('pipe'));
  const torchLight = pipeAndTorchLights.find(l => l.name.includes('torch'))! as THREE.Mesh;
  (torchLight.material as THREE.MeshBasicMaterial).color = (
    torchLight.material as THREE.MeshBasicMaterial
  ).color.lerp(new THREE.Color(0xf7c559), 0.4);
  pipeLightBloomEffect.selection.set(pipeAndTorchLights);

  const smaaEffect2 = new SMAAEffect({
    preset: {
      [GraphicsQuality.Low]: SMAAPreset.LOW,
      [GraphicsQuality.Medium]: SMAAPreset.MEDIUM,
      [GraphicsQuality.High]: SMAAPreset.HIGH,
    }[quality],
  });
  const fxPass = new EffectPass(viz.camera, pipeLightBloomEffect, smaaEffect2);
  effectComposer.addPass(fxPass);

  viz.renderer.toneMapping = THREE.CineonToneMapping;
  viz.renderer.toneMappingExposure = 1.8;

  const averagePipeLightPos = pipeAndTorchLights
    .filter(l => l.name.includes('pipe'))
    .reduce((acc, light) => acc.add(light.position), new THREE.Vector3())
    .divideScalar(pipeAndTorchLights.length);
  const torchLightPos = pipeAndTorchLights.find(l => l.name.includes('torch'))!.position;
  const black = new THREE.Color(0x0);
  const baseBGColor = new THREE.Color(0x8f4509);
  let lastDownFactor = Infinity;
  viz.registerBeforeRenderCb(curTimeSecs => {
    const distanceToPipeLights = viz.camera.position.distanceTo(averagePipeLightPos);
    const distanceToTorchLight = viz.camera.position.distanceTo(torchLightPos);
    // when player is < 60 meters away:
    //  * intensity = 23
    //  * scale multiplier = 1
    // when player is 130 meters away:
    //  * intensity = 1
    //  * scale multiplier = 12

    const pipeLightScaleMultiplier = smoothstep(60, 130, distanceToPipeLights) * 11 + 1;
    const pipeLightIntensity = (1 - smoothstep(60, 130, distanceToPipeLights)) * 22 + 1;
    pipeLightBloomEffect.intensity = pipeLightIntensity;
    const torchLightScaleMultiplier = smoothstep(20, 90, distanceToTorchLight) * 1 + 1;

    let pulse = Math.sin(curTimeSecs * 2);
    // sharpen the curve of the pulse a bit
    pulse = Math.sign(pulse) * Math.pow(Math.abs(pulse), 0.7);
    const pipeLightScale = (pulse * 0.2 + 0.4) * pipeLightScaleMultiplier;
    for (const pipeLight of pipeLights) {
      pipeLight.scale.set(pipeLightScale, pipeLightScale, pipeLightScale);
    }
    const torchLightScale = (pulse * 0.4 + 1.4) * torchLightScaleMultiplier;
    torchLight.scale.set(torchLightScale, torchLightScale, torchLightScale);

    // fade everything to black as player descends
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
  let didRender = false;
  viz.setRenderOverride(timeDiffSeconds => {
    const newN8AOIntensity = computeN8AOIntensity(viz.camera.position);
    if (n8aoPass && newN8AOIntensity !== lastN8AOIntensity) {
      n8aoPass.configuration.intensity = newN8AOIntensity;
      lastN8AOIntensity = newN8AOIntensity;

      if (newN8AOIntensity > 0 && !n8aoPassEnabled) {
        effectComposer.addPass(n8aoPass, USE_DEPTH_PREPASS ? 2 : 1);
        n8aoPassEnabled = true;
      } else if (newN8AOIntensity === 0 && n8aoPassEnabled) {
        effectComposer.removePass(n8aoPass);
        n8aoPassEnabled = false;
      }
    }

    effectComposer.render(timeDiffSeconds);
    viz.renderer.shadowMap.autoUpdate = false;
    viz.renderer.shadowMap.needsUpdate = false;
    // For some reason, the shadow map that we render at the start of everything is getting cleared at some
    // point during the setup of this postprocessing pipeline.
    //
    // So, we have to re-populate the shadowmap so that it can be used to power the godrays and, well, shadows.
    if (!didRender) {
      didRender = true;
      onFirstRender();
    }
  });

  viz.registerResizeCb(() => {
    effectComposer.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
  });
};
