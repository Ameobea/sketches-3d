import { MipmapBlurPass, Pass } from 'postprocessing';
import * as THREE from 'three';
import THRESHOLD_FRAG from './shaders/emissiveThreshold.frag?raw';
import THRESHOLD_VERT from './shaders/emissiveThreshold.vert?raw';
import type { Viz } from '..';

export interface EmissiveBloomConfig {
  /**
   * Number of mip levels in the downsample/upsample chain.
   * More levels = wider maximum glow spread, but more GPU cost.
   * Quality-based defaults are applied in defaultPostprocessing.ts.
   */
  levels?: number;
  /**
   * Blend radius during upsampling (0–1).
   * Lower values = tighter per-feature glow; higher = broader halo.
   * Default 0.85 (MipmapBlurPass default).
   */
  radius?: number;
  /**
   * Multiplier applied to the bloom texture before additive compositing in FinalPass.
   * Default 1.0.
   */
  intensity?: number;
  /**
   * Luminance threshold for bloom gating (0–1).
   *
   * Pixels with luminance below this value are suppressed before blurring, so only
   * bright areas (e.g. the lit strands of an animated portal) bloom. Pixels above
   * threshold + smoothing contribute fully; the transition is a smoothstep.
   *
   * Omit or set to 0 to disable.
   */
  luminanceThreshold?: number;
  /**
   * Width of the smoothstep transition around luminanceThreshold.
   * Default 0.1. Larger values give a softer roll-off into the bloom.
   * Ignored when `luminanceSoftKnee` is set.
   */
  luminanceSmoothing?: number;
  /**
   * When > 0, switches the gate from a smoothstep to a UE4-style soft-knee: a
   * quadratic ramp of width `2 * luminanceSoftKnee` centered on `luminanceThreshold`,
   * then a subtractive linear region above it.
   */
  luminanceSoftKnee?: number;
}

/**
 * Bloom pass for the emissive bypass layer, with an optional pre-filter step.
 *
 * Pipeline:
 *   sourceRT → [filter pass] → MipmapBlur → bloomTexture
 *
 * The filter pass runs whenever fog and/or a luminance threshold is configured.
 * It applies fog first (so blurred halos correctly attenuate with distance) and
 * then the threshold ramp (so only bright pixels contribute to bloom). When
 * neither is configured, MipmapBlur reads sourceRT directly and the filter RT
 * is never allocated.
 *
 * needsSwap = false — writes to its own internal targets.
 */
export class EmissiveBloomPass extends Pass {
  private readonly viz: Viz | null;
  private readonly mipmapBlur: MipmapBlurPass;
  private readonly sourceRT: THREE.WebGLRenderTarget;
  readonly intensity: number;

  private readonly filterRT: THREE.WebGLRenderTarget | null = null;
  private readonly filterMat: THREE.ShaderMaterial | null = null;
  private readonly hasFog: boolean;

  /** Output texture: blurred bloom glow. Pass directly to FinalPass as emissiveBloomBuffer. */
  get bloomTexture(): THREE.Texture {
    return this.mipmapBlur.texture;
  }

  public setRadius(value: number): void {
    this.mipmapBlur.radius = value;
  }

  public setLuminanceThreshold(value: number): void {
    if (this.filterMat?.uniforms.threshold) {
      this.filterMat.uniforms.threshold.value = value;
    }
  }

  public setLuminanceSmoothing(value: number): void {
    if (this.filterMat?.uniforms.smoothing) {
      this.filterMat.uniforms.smoothing.value = value;
    }
  }

  public setLuminanceSoftKnee(value: number): void {
    if (this.filterMat?.uniforms.softKnee) {
      this.filterMat.uniforms.softKnee.value = value;
    }
  }

  override setDepthTexture(depthTexture: THREE.Texture | null): void {
    if (this.filterMat?.uniforms.depthBuffer) {
      this.filterMat.uniforms.depthBuffer.value = depthTexture;
    }
  }

  constructor(
    sourceRT: THREE.WebGLRenderTarget,
    config: EmissiveBloomConfig = {},
    fogShader?: string,
    viz?: Viz
  ) {
    super('EmissiveBloomPass');
    this.needsSwap = false;
    this.sourceRT = sourceRT;
    this.viz = viz ?? null;
    this.intensity = config.intensity ?? 1.0;
    this.hasFog = !!fogShader && !!viz;

    this.mipmapBlur = new MipmapBlurPass();
    // `levels` must be set before setSize() is called (it rebuilds the mipmap chain)
    this.mipmapBlur.levels = config.levels ?? 8;
    this.mipmapBlur.radius = config.radius ?? 0.85;

    const thresh = config.luminanceThreshold ?? 0;
    const hasThreshold = thresh > 0;

    if (hasThreshold || this.hasFog) {
      this.filterRT = new THREE.WebGLRenderTarget(sourceRT.width, sourceRT.height, {
        type: THREE.HalfFloatType,
        format: THREE.RGBAFormat,
        depthBuffer: false,
      });

      const defines: Record<string, string> = {};
      if (hasThreshold) defines.HAS_THRESHOLD = '1';
      if (this.hasFog) defines.HAS_FOG = '1';

      const uniforms: Record<string, THREE.IUniform> = {
        tInput: { value: null },
      };
      if (hasThreshold) {
        uniforms.threshold = { value: thresh };
        uniforms.smoothing = { value: config.luminanceSmoothing ?? 0.1 };
        uniforms.softKnee = { value: config.luminanceSoftKnee ?? 0 };
      }
      if (this.hasFog && viz) {
        uniforms.depthBuffer = { value: null };
        uniforms.projectionMatrixInverse = { value: viz.camera.projectionMatrixInverse };
        uniforms.cameraWorldMatrix = { value: viz.camera.matrixWorld };
        uniforms.fogCameraPos = { value: new THREE.Vector3() };
        uniforms.fogPlayerPos = { value: new THREE.Vector3() };
        uniforms.curTimeSeconds = { value: 0.0 };
      }

      this.filterMat = new THREE.ShaderMaterial({
        name: 'EmissiveBloomFilterMaterial',
        uniforms,
        defines,
        vertexShader: THRESHOLD_VERT,
        fragmentShader: this.hasFog ? `${fogShader}\n${THRESHOLD_FRAG}` : THRESHOLD_FRAG,
        depthWrite: false,
        depthTest: false,
      });
      // Prime the Pass's internal fullscreen quad with this material so this.scene
      // has the quad ready for the filter render in render(). Also lets the base
      // Pass.setDepthTexture wire `depthBuffer` automatically.
      this.fullscreenMaterial = this.filterMat;

      if (this.hasFog) {
        this.needsDepthTexture = true;
      }
    }
  }

  override initialize(renderer: THREE.WebGLRenderer, alpha: boolean, frameBufferType: number): void {
    // Forward to the inner pass so it sets HalfFloat on its internal mipmaps.
    this.mipmapBlur.initialize(renderer, alpha, THREE.HalfFloatType);
    // Pre-compile the filter material so the first frame doesn't stall on shader compilation.
    if (this.filterMat) {
      renderer.compile(this.scene, this.camera);
    }
  }

  override setSize(width: number, height: number): void {
    this.filterRT?.setSize(width, height);
    this.mipmapBlur.setSize(width, height);
  }

  override render(
    renderer: THREE.WebGLRenderer,
    _inputBuffer: THREE.WebGLRenderTarget,
    _outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number
  ): void {
    let blurInput = this.sourceRT;

    if (this.filterMat && this.filterRT) {
      this.filterMat.uniforms.tInput.value = this.sourceRT.texture;
      if (this.hasFog && this.viz) {
        this.filterMat.uniforms.fogCameraPos.value.setFromMatrixPosition(this.viz.camera.matrixWorld);
        if (this.viz.fpCtx) {
          this.filterMat.uniforms.curTimeSeconds.value = this.viz.fpCtx.getPhysicsTime();
          const playerPos = this.viz.fpCtx.playerController.getPosition();
          if (playerPos) {
            this.filterMat.uniforms.fogPlayerPos.value.set(playerPos.x(), playerPos.y(), playerPos.z());
          }
        }
      }
      renderer.setRenderTarget(this.filterRT);
      renderer.clear();
      renderer.render(this.scene, this.camera);
      blurInput = this.filterRT;
    }

    this.mipmapBlur.render(renderer, blurInput, null, deltaTime);
  }

  override dispose(): void {
    this.filterRT?.dispose();
    this.filterMat?.dispose();
    this.mipmapBlur.dispose();
    super.dispose();
  }
}
