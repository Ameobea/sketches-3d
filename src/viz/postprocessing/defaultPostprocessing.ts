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
import { buildOcclusionDepthMaterial, buildPlainDepthMaterial } from 'src/viz/shaders/customShader';
import { EmissiveBypassPass, EMISSIVE_BYPASS_LAYER } from 'src/viz/passes/emissiveBypassPass';
import { EmissiveBloomPass, type EmissiveBloomConfig } from 'src/viz/passes/emissiveBlurPass';
import { FinalPass, type ToneMappingMode } from 'src/viz/passes/finalPass';
import { StableDepthEffectComposer } from 'src/viz/passes/stableDepthComposer';
import type { SkyStack } from 'src/viz/SkyStack';

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
  private readonly renderFrameCb: (timeDiffSeconds: number) => void;

  constructor(
    effectComposer: StableDepthEffectComposer,
    depthPass: DepthPass | null,
    depthPrePassMaterial: THREE.Material | null,
    renderer: THREE.WebGLRenderer,
    renderFrameCb: (timeDiffSeconds: number) => void,
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
    this.renderFrameCb = renderFrameCb;
  }

  get hasFinalPass(): boolean {
    return this.finalPass !== null;
  }

  setGamma(gamma: number): void {
    this.finalPass?.setGamma(gamma);
  }

  renderFrame(timeDiffSeconds: number): void {
    this.renderFrameCb(timeDiffSeconds);
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
    luminanceSoftKnee,
  }: {
    radius?: number;
    intensity?: number;
    luminanceThreshold?: number;
    luminanceSmoothing?: number;
    luminanceSoftKnee?: number;
  }): void {
    if (radius !== undefined) this.emissiveBloomPass?.setRadius(radius);
    if (intensity !== undefined) this.finalPass?.setBloomIntensity(intensity);
    if (luminanceThreshold !== undefined) this.emissiveBloomPass?.setLuminanceThreshold(luminanceThreshold);
    if (luminanceSmoothing !== undefined) this.emissiveBloomPass?.setLuminanceSmoothing(luminanceSmoothing);
    if (luminanceSoftKnee !== undefined) this.emissiveBloomPass?.setLuminanceSoftKnee(luminanceSoftKnee);
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
   * Config for the emissive bloom pass. Only used when `emissiveBypass` is true.
   * `levels` defaults to a quality-dependent value if not set:
   *   Low → 4, Medium → 6, High → 8.
   * Pass `null` to disable the bloom pass entirely (emissive bypass compositing still runs).
   */
  emissiveBloom?: EmissiveBloomConfig | null;
  /**
   * Intensity of the dedicated ambient light used exclusively by the emissive bypass render
   * (layer 31). Decouples portal/bypass-mesh brightness from the main scene's ambient light
   * so that dimming scene lights does not affect emissive bypass objects.
   * Only relevant when `emissiveBypass` is true. Default: 2.8 (nexus baseline).
   */
  emissiveBypassAmbientIntensity?: number;
  /** See doc comment of `FinalPass` for usage of this. */
  fogShader?: string;
  /**
   * When true, fragments at the depth-buffer far plane skip tone mapping in FinalPass,
   * so sky shaders' authored colors are preserved 1:1. See `FinalPass` for details.
   */
  skyBypassTonemap?: boolean;
  /**
   * Unified sky sub-pipeline. When provided, its pass is inserted between the depth
   * pre-pass and the main render pass; it owns the emissive RT that is then shared
   * with the emissive bypass pass (bypass meshes render on top of sky content).
   * Requires `useDepthPrePass: true` and `emissiveBypass: true`.
   */
  skyStack?: SkyStack;
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
  fogShader,
  skyBypassTonemap = false,
  skyStack,
}: ConfigureDefaultPostprocessingPipelineParams): PostprocessingPipelineController => {
  if (skyStack) {
    if (!useDepthPrePass) {
      throw new Error('skyStack requires useDepthPrePass: true (needs stable depth target).');
    }
    if (!emissiveBypass) {
      throw new Error('skyStack requires emissiveBypass: true (sky RT feeds the emissive composite).');
    }
  }
  const effectComposer = new StableDepthEffectComposer(viz.renderer, {
    multisampling: 0,
    frameBufferType: THREE.HalfFloatType,
  });

  let renderPass: MainRenderPass | RenderPass;
  let depthPass: DepthPass | null = null;
  let depthPrePassMaterial: THREE.Material | null = null;

  if (useDepthPrePass) {
    viz.renderer.autoClear = false;
    viz.renderer.autoClearColor = true;
    viz.renderer.autoClearDepth = false;
    depthPrePassMaterial = buildOcclusionDepthMaterial();
    depthPrePassMaterial.side = THREE.FrontSide;
    const plainDepthMaterial = buildPlainDepthMaterial();
    plainDepthMaterial.side = THREE.FrontSide;
    depthPass = new DepthPass(viz.scene, viz.camera, depthPrePassMaterial, false, plainDepthMaterial);
    depthPass.skipShadowMapUpdate = true;
    effectComposer.addPass(depthPass);

    const mainRenderPass = new MainRenderPass(viz.scene, viz.camera);
    mainRenderPass.skipShadowMapUpdate = !autoUpdateShadowMap;
    mainRenderPass.needsDepthTexture = true;
    effectComposer.addPass(mainRenderPass);
    renderPass = mainRenderPass;

    if (skyStack) {
      const stableDepthTgt = effectComposer.stableDepthTarget;
      if (!stableDepthTgt) {
        throw new Error('skyStack: stableDepthTarget was not created after adding MainRenderPass.');
      }
      if (!stableDepthTgt.depthTexture) {
        throw new Error('skyStack: stableDepthTarget has no depthTexture.');
      }
      skyStack.setSceneDepth(stableDepthTgt.depthTexture);
      // Share stableDepth's depthTexture as emissiveRT's depth attachment so
      // EmissiveBypassPass depth-tests bypass meshes without a per-frame depth blit.
      skyStack.pass.setEmissiveDepthTexture(stableDepthTgt.depthTexture as THREE.DepthTexture);
      viz.scene.background = null;
      effectComposer.addPass(skyStack.pass, 1);
    }
  } else {
    renderPass = new RenderPass(viz.scene, viz.camera);
    effectComposer.addPass(renderPass);
  }

  addMiddlePasses?.(effectComposer, viz, quality);

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
      height,
      skyStack?.emissiveRT
    );

    const stableDepthTgt = effectComposer.stableDepthTarget;
    if (!stableDepthTgt) {
      throw new Error(
        'emissiveBypass requires a depth source. Enable useDepthPrePass=true or add a fogShader to ensure a depth texture is allocated before the emissive bypass pass.'
      );
    }
    // Share the composer's stable depth as emissiveRT's depth attachment.
    // No-op when SkyStack provided the external RT (it wired its own depth above).
    emissiveBypassPass.setStableDepthTexture(stableDepthTgt.depthTexture as THREE.DepthTexture);

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
      emissiveBlurPass = new EmissiveBloomPass(emissiveBypassPass.emissiveRT, bloomConfig, fogShader, viz);
      effectComposer.addPass(emissiveBlurPass);
    }

    // emissive pass objects shouldn't be impacted by the main scene's lighting, so we create a
    // dedicated static light just for that pass.
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

  // we do our own tone mapping in `FinalPass`
  viz.renderer.toneMapping = THREE.NoToneMapping;

  const finalEmissiveTexture = emissiveBypassPass?.emissiveRT.texture ?? null;

  const finalPass = new FinalPass(viz, {
    toneMapping: toneMapping.mode ?? 'aces',
    exposure: toneMapping.exposure ?? 1.0,
    emissiveBuffer: finalEmissiveTexture,
    emissiveBloomBuffer: emissiveBlurPass?.bloomTexture ?? null,
    bloomIntensity: emissiveBlurPass?.intensity ?? 1.0,
    fogShader,
    skyBypassTonemap,
  });
  effectComposer.addPass(finalPass);

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

  let didRenderShadowMap = false;
  let sceneGeomReady = false;
  viz.awaitPhysicsStartupBarriers().then(() => {
    sceneGeomReady = true;
  });

  viz.renderer.shadowMap.autoUpdate = autoUpdateShadowMap;
  viz.renderer.shadowMap.needsUpdate = autoUpdateShadowMap;
  const renderFrame = (timeDiffSeconds: number) => {
    if (!didRenderShadowMap && viz.renderer.shadowMap.enabled && !autoUpdateShadowMap && sceneGeomReady) {
      didRenderShadowMap = true;
      viz.renderer.shadowMap.needsUpdate = true;
      viz.scene.traverse(obj => {
        if (!(obj instanceof THREE.DirectionalLight) || !obj.castShadow) {
          return;
        }
        obj.shadow.camera.updateProjectionMatrix();
        obj.shadow.needsUpdate = true;
      });
      effectComposer.render(timeDiffSeconds);
      viz.scene.traverse(obj => {
        if (!(obj instanceof THREE.DirectionalLight)) {
          return;
        }
        obj.shadow.needsUpdate = false;
        obj.shadow.autoUpdate = false;
      });
      viz.renderer.shadowMap.autoUpdate = false;
    }

    effectComposer.render(timeDiffSeconds);
  };
  viz.setRenderOverride(renderFrame);

  const controller = new PostprocessingPipelineController(
    effectComposer,
    depthPass,
    depthPrePassMaterial,
    viz.renderer,
    renderFrame,
    emissiveBypassPass,
    emissiveBlurPass,
    finalPass
  );
  viz.postprocessingController = controller;
  viz.registerResizeCb(() => {
    const logicalSize = viz.renderer.getSize(new THREE.Vector2());
    effectComposer.setSize(logicalSize.x, logicalSize.y);
  });
  controller.setGamma(viz.vizConfig.current.graphics.gamma);
  return controller;
};
