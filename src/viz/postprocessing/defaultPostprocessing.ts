import {
  type EffectComposer,
  EffectPass,
  RenderPass,
  SMAAEffect,
  SMAAPreset,
  type Effect,
} from 'postprocessing';
import * as THREE from 'three';

import type { Viz } from 'src/viz';
import type { PostprocessingController } from 'src/viz';
import { GraphicsQuality } from 'src/viz/conf';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';
import { buildOcclusionDepthMaterial } from 'src/viz/shaders/customShader';
import { EmissiveBypassPass, EMISSIVE_BYPASS_LAYER } from 'src/viz/passes/emissiveBypassPass';
import { EmissiveBloomPass, type EmissiveBloomConfig } from 'src/viz/passes/emissiveBlurPass';
import { FinalPass, type ToneMappingMode } from 'src/viz/passes/finalPass';
import { StableDepthEffectComposer } from 'src/viz/passes/stableDepthComposer';

const populateShadowMap = (viz: Viz, autoUpdateShadowMap: boolean) => {
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
  if (!autoUpdateShadowMap) {
    shadows.forEach(shadow => {
      shadow.needsUpdate = false;
      shadow.autoUpdate = false;
    });
    viz.renderer.shadowMap.needsUpdate = false;
    viz.renderer.shadowMap.autoUpdate = false;
  }
  viz.renderer.shadowMap.enabled = true;
};

/**
 * Default emissive bloom configuration (quality-independent params).
 * `levels` is intentionally excluded — it's set per-quality in the pipeline.
 * Scenes can override individual fields via the `emissiveBloom` param.
 */
export const DEFAULT_EMISSIVE_BLOOM_CONFIG: Omit<EmissiveBloomConfig, 'levels'> = {
  radius: 0.35,
  intensity: 0.8,
  luminanceThreshold: 0,
  luminanceSmoothing: 0,
};

export class PostprocessingPipelineController implements PostprocessingController {
  public effectComposer: StableDepthEffectComposer;
  public depthPass: DepthPass | null;
  public depthPrePassMaterial: THREE.Material | null;
  public renderer: THREE.WebGLRenderer;
  public readonly emissiveBypassPass: EmissiveBypassPass | null;
  private readonly emissiveBloomPass: EmissiveBloomPass | null;
  private readonly finalPass: FinalPass | null;

  constructor(
    effectComposer: StableDepthEffectComposer,
    depthPass: DepthPass | null,
    depthPrePassMaterial: THREE.Material | null,
    renderer: THREE.WebGLRenderer,
    emissiveBypassPass: EmissiveBypassPass | null = null,
    emissiveBloomPass: EmissiveBloomPass | null = null,
    finalPass: FinalPass | null = null
  ) {
    this.effectComposer = effectComposer;
    this.depthPass = depthPass;
    this.depthPrePassMaterial = depthPrePassMaterial;
    this.renderer = renderer;
    this.emissiveBypassPass = emissiveBypassPass;
    this.emissiveBloomPass = emissiveBloomPass;
    this.finalPass = finalPass;
  }

  get hasFinalPass(): boolean {
    return this.finalPass !== null;
  }

  setGamma(gamma: number): void {
    this.finalPass?.setGamma(gamma);
  }

  setDepthPrePassEnabled(enabled: boolean) {
    if (this.depthPass) {
      this.depthPass.enabled = enabled;
    }
    this.renderer.autoClearDepth = !enabled;
  }

  addEmissiveBypassObject(mesh: THREE.Mesh): void {
    this.emissiveBypassPass?.addBypassMesh(mesh);
  }

  /**
   * Dynamically update emissive bloom parameters. Safe to call every frame.
   * `radius` controls blur spread (MipmapBlurPass upsampling blend — only touches a uniform).
   * `intensity` scales the additive bloom contribution in FinalPass.
   * `luminanceThreshold` and `luminanceSmoothing` control the threshold pass that runs before the blur (if enabled). Higher threshold means fewer pixels contribute to bloom; smoothing controls how gradually pixels near the threshold contribute.
   */
  /**
   * Scan `scene` for any meshes whose material has `userData.emissiveBypass` set
   * and register them with the emissive bypass pass. Safe to call multiple times —
   * already-registered meshes are skipped. Use this when bypass meshes are added
   * to the scene after the initial first-frame auto-scan (e.g. deferred async setup).
   */
  rescanBypassMeshes(scene: THREE.Scene): void {
    if (!this.emissiveBypassPass) return;
    scene.traverse(obj => {
      if (!(obj instanceof THREE.Mesh)) return;
      const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
      if (mats.some(m => m?.userData?.emissiveBypass)) {
        this.emissiveBypassPass!.addBypassMesh(obj);
      }
    });
  }

  setEmissiveBloom({
    radius,
    intensity,
    luminanceThreshold,
    luminanceSmoothing,
  }: {
    radius?: number;
    intensity?: number;
    luminanceThreshold?: number;
    luminanceSmoothing?: number;
  }): void {
    if (radius !== undefined) this.emissiveBloomPass?.setRadius(radius);
    if (intensity !== undefined) this.finalPass?.setBloomIntensity(intensity);
    if (luminanceThreshold !== undefined) this.emissiveBloomPass?.setLuminanceThreshold(luminanceThreshold);
    if (luminanceSmoothing !== undefined) this.emissiveBloomPass?.setLuminanceSmoothing(luminanceSmoothing);
  }
}

export interface ToneMappingConfig {
  mode?: ToneMappingMode;
  exposure?: number;
}

export interface ConfigureDefaultPostprocessingPipelineParams {
  viz: Viz;
  quality: GraphicsQuality;
  addMiddlePasses?: (composer: EffectComposer, viz: Viz, quality: GraphicsQuality) => void;
  toneMapping?: ToneMappingConfig;
  postEffects?: Effect[];
  autoUpdateShadowMap?: boolean;
  enableAntiAliasing?: boolean;
  useDepthPrePass?: boolean;
  emissiveBypass?: boolean;
  /**
   * Config for the emissive bloom pass. Only used when emissiveBypass is true.
   * `levels` defaults to a quality-dependent value if not set:
   *   Low → 4, Medium → 6, High → 8.
   * Pass `null` to disable the bloom pass entirely (emissive bypass compositing still runs).
   */
  emissiveBloom?: EmissiveBloomConfig | null;
  /**
   * Intensity of the dedicated ambient light used exclusively by the emissive bypass render
   * (layer 31). Decouples portal/bypass-mesh brightness from the main scene's ambient light
   * so that dimming scene lights does not affect emissive bypass objects.
   * Only relevant when emissiveBypass is true. Default: 2.8 (nexus baseline).
   */
  emissiveBypassAmbientIntensity?: number;
}

export const configureDefaultPostprocessingPipeline = ({
  viz,
  quality,
  addMiddlePasses,
  toneMapping = {},
  postEffects,
  autoUpdateShadowMap = false,
  enableAntiAliasing = true,
  useDepthPrePass = true,
  emissiveBypass = false,
  emissiveBloom = {} as EmissiveBloomConfig | null,
  emissiveBypassAmbientIntensity = 2.8,
}: ConfigureDefaultPostprocessingPipelineParams): PostprocessingPipelineController => {
  const effectComposer = new StableDepthEffectComposer(viz.renderer, {
    multisampling: 0,
    frameBufferType: THREE.HalfFloatType,
  });

  let depthPass: DepthPass | null = null;
  let depthPrePassMaterial: THREE.Material | null = null;

  if (useDepthPrePass) {
    viz.renderer.autoClear = false;
    viz.renderer.autoClearColor = true;
    viz.renderer.autoClearDepth = false;
    depthPrePassMaterial = buildOcclusionDepthMaterial();
    depthPrePassMaterial.side = THREE.FrontSide;
    depthPass = new DepthPass(viz.scene, viz.camera, depthPrePassMaterial);
    depthPass.skipShadowMapUpdate = true;
    effectComposer.addPass(depthPass);

    const renderPass = new MainRenderPass(viz.scene, viz.camera);
    renderPass.skipShadowMapUpdate = true;
    renderPass.needsDepthTexture = true;
    effectComposer.addPass(renderPass);
  } else {
    const renderPass = new RenderPass(viz.scene, viz.camera);
    effectComposer.addPass(renderPass);
  }

  addMiddlePasses?.(effectComposer, viz, quality);

  // HDR post effects run before tone mapping
  if (postEffects?.length) {
    const hdrFxPass = new EffectPass(viz.camera, ...postEffects);
    effectComposer.addPass(hdrFxPass);
  }

  let emissiveBypassPass: EmissiveBypassPass | null = null;
  let emissiveBlurPass: EmissiveBloomPass | null = null;
  if (emissiveBypass) {
    const { width, height } = viz.renderer.domElement;
    emissiveBypassPass = new EmissiveBypassPass(
      viz.scene,
      viz.camera as THREE.PerspectiveCamera,
      width,
      height
    );

    const stableDepthTgt = effectComposer.stableDepthTarget;
    if (stableDepthTgt) {
      emissiveBypassPass.setStableDepthTarget(stableDepthTgt);
    }

    effectComposer.addPass(emissiveBypassPass);

    if (emissiveBloom !== null) {
      const qualityLevels = {
        [GraphicsQuality.Low]: 3,
        [GraphicsQuality.Medium]: 5,
        [GraphicsQuality.High]: 6,
      };
      const bloomConfig: EmissiveBloomConfig = {
        levels: qualityLevels[quality],
        ...DEFAULT_EMISSIVE_BLOOM_CONFIG,
        ...emissiveBloom,
      };
      emissiveBlurPass = new EmissiveBloomPass(emissiveBypassPass.emissiveRT, bloomConfig);
      effectComposer.addPass(emissiveBlurPass);
    }

    // Dedicated ambient light for the bypass render only (layer 31).
    // Using a layer-isolated light keeps portal/emissive-bypass brightness constant
    // regardless of what the main scene's ambient/directional lights are doing — so
    // e.g. dimming scene lights for a proximity effect doesn't dim the portals.
    const bypassAmbientLight = new THREE.AmbientLight(0xffffff, emissiveBypassAmbientIntensity);
    bypassAmbientLight.layers.disableAll();
    bypassAmbientLight.layers.enable(EMISSIVE_BYPASS_LAYER);
    viz.scene.add(bypassAmbientLight);

    // Defer mesh layer assignment to the first render frame. By then viz.scene.add(loadedWorld)
    // (and any other scene population) has completed, regardless of which setup path was used.
    let autoAssigned = false;
    viz.registerBeforeRenderCb(() => {
      if (autoAssigned) {
        return;
      }

      autoAssigned = true;
      viz.scene.traverse(obj => {
        if (!(obj instanceof THREE.Mesh)) {
          return;
        }

        const mats = Array.isArray(obj.material) ? obj.material : [obj.material];
        if (mats.some(m => m?.userData?.emissiveBypass)) {
          emissiveBypassPass!.addBypassMesh(obj);
        }
      });
    });
  }

  // Disable in-shader tone mapping. The FinalPass applies tone mapping + sRGB encoding
  // to the full composited scene in one place.
  viz.renderer.toneMapping = THREE.NoToneMapping;

  const finalPass = new FinalPass({
    toneMapping: toneMapping.mode ?? 'aces',
    exposure: toneMapping.exposure ?? 1.0,
    emissiveBuffer: emissiveBypassPass?.emissiveRT.texture ?? null,
    emissiveBloomBuffer: emissiveBlurPass?.bloomTexture ?? null,
    bloomIntensity: emissiveBlurPass?.intensity ?? 1.0,
  });
  effectComposer.addPass(finalPass);

  // SMAA runs after tone mapping + sRGB encode so edge detection operates in display
  // space, where luminance contrast matches perception.
  if (enableAntiAliasing) {
    const smaaEffect = new SMAAEffect({
      preset: {
        [GraphicsQuality.Low]: SMAAPreset.LOW,
        [GraphicsQuality.Medium]: SMAAPreset.MEDIUM,
        [GraphicsQuality.High]: SMAAPreset.HIGH,
      }[quality],
    });
    const smaaPass = new EffectPass(viz.camera, smaaEffect);
    smaaPass.renderToScreen = true;
    effectComposer.addPass(smaaPass);
  } else {
    finalPass.renderToScreen = true;
  }

  let didRender = false;
  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
    viz.renderer.shadowMap.autoUpdate = autoUpdateShadowMap;
    viz.renderer.shadowMap.needsUpdate = autoUpdateShadowMap;

    // For some reason, the shadow map that we render at the start of everything is getting cleared at some
    // point during the setup of this postprocessing pipeline.
    //
    // So, we have to re-populate the shadowmap so that it can be used to power the godrays and, well, shadows.
    if (!didRender && viz.renderer.shadowMap.enabled && !autoUpdateShadowMap) {
      didRender = true;
      populateShadowMap(viz, autoUpdateShadowMap);
    }
  });

  const controller = new PostprocessingPipelineController(
    effectComposer,
    depthPass,
    depthPrePassMaterial,
    viz.renderer,
    emissiveBypassPass,
    emissiveBlurPass,
    finalPass
  );
  viz.postprocessingController = controller;
  viz.registerResizeCb(() => {
    effectComposer.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
  });
  controller.setGamma(viz.vizConfig.current.graphics.gamma);
  return controller;
};
