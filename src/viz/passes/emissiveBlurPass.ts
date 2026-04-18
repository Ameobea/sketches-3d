import { MipmapBlurPass, Pass } from 'postprocessing';
import * as THREE from 'three';
import THRESHOLD_FRAG from './shaders/emissiveThreshold.frag?raw';
import THRESHOLD_VERT from './shaders/emissiveThreshold.vert?raw';

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
   * Pixels with luminance below this value are suppressed before blurring, so only
   * bright areas (e.g. the lit strands of an animated portal) bloom. Pixels above
   * threshold + smoothing contribute fully; the transition is a smoothstep.
   * Omit or set to 0 to disable (every emissive pixel blooms equally).
   */
  luminanceThreshold?: number;
  /**
   * Width of the smoothstep transition around luminanceThreshold.
   * Default 0.1. Larger values give a softer roll-off into the bloom.
   */
  luminanceSmoothing?: number;
}

/**
 * Bloom pass for the emissive bypass layer.
 * Wraps postprocessing's MipmapBlurPass (the same high-quality downsample/upsample
 * chain used by BloomEffect) to blur the emissive render target and produce a
 * dedicated bloom texture that FinalPass composites additively.
 *
 * Optionally applies a luminance threshold pass before blurring so that only the
 * bright areas of animated emissive content (e.g. portal strand highlights) bloom.
 *
 * needsSwap = false — writes to its own internal targets.
 */
export class EmissiveBloomPass extends Pass {
  private readonly mipmapBlur: MipmapBlurPass;
  private readonly sourceRT: THREE.WebGLRenderTarget;
  readonly intensity: number;

  // Threshold pass — only allocated when luminanceThreshold is configured.
  private readonly thresholdRT: THREE.WebGLRenderTarget | null = null;
  private readonly thresholdMat: THREE.ShaderMaterial | null = null;

  /** Output texture: blurred bloom glow. Pass directly to FinalPass as emissiveBloomBuffer. */
  get bloomTexture(): THREE.Texture {
    return this.mipmapBlur.texture;
  }

  /**
   * Update the blur radius without rebuilding the mipmap chain.
   * Safe to call every frame — only touches a shader uniform.
   */
  public setRadius(value: number): void {
    this.mipmapBlur.radius = value;
  }

  public setLuminanceThreshold(value: number): void {
    if (this.thresholdMat) {
      this.thresholdMat.uniforms.threshold.value = value;
    }
  }

  public setLuminanceSmoothing(value: number): void {
    if (this.thresholdMat) {
      this.thresholdMat.uniforms.smoothing.value = value;
    }
  }

  constructor(sourceRT: THREE.WebGLRenderTarget, config: EmissiveBloomConfig = {}) {
    super('EmissiveBloomPass');
    this.needsSwap = false;
    this.sourceRT = sourceRT;
    this.intensity = config.intensity ?? 1.0;

    this.mipmapBlur = new MipmapBlurPass();
    // levels must be set before setSize() is called (it rebuilds the mipmap chain)
    this.mipmapBlur.levels = config.levels ?? 8;
    this.mipmapBlur.radius = config.radius ?? 0.85;

    const thresh = config.luminanceThreshold ?? 0;
    if (thresh > 0) {
      this.thresholdRT = new THREE.WebGLRenderTarget(sourceRT.width, sourceRT.height, {
        type: THREE.HalfFloatType,
        format: THREE.RGBAFormat,
        depthBuffer: false,
      });
      this.thresholdMat = new THREE.ShaderMaterial({
        uniforms: {
          tInput: { value: null },
          threshold: { value: thresh },
          smoothing: { value: config.luminanceSmoothing ?? 0.1 },
        },
        vertexShader: THRESHOLD_VERT,
        fragmentShader: THRESHOLD_FRAG,
        depthWrite: false,
        depthTest: false,
      });
      // Prime the Pass's internal fullscreen quad with this material so this.scene
      // has the quad ready for the threshold render in render().
      this.fullscreenMaterial = this.thresholdMat;
    }
  }

  override initialize(renderer: THREE.WebGLRenderer, alpha: boolean, frameBufferType: number): void {
    // Forward to the inner pass so it sets HalfFloat on its internal mipmaps.
    this.mipmapBlur.initialize(renderer, alpha, THREE.HalfFloatType);
    // Pre-compile the threshold material so the first frame doesn't stall on shader compilation.
    if (this.thresholdMat) {
      renderer.compile(this.scene, this.camera);
    }
  }

  override setSize(width: number, height: number): void {
    this.thresholdRT?.setSize(width, height);
    this.mipmapBlur.setSize(width, height);
  }

  override render(
    renderer: THREE.WebGLRenderer,
    _inputBuffer: THREE.WebGLRenderTarget,
    _outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number
  ): void {
    let blurInput = this.sourceRT;

    if (this.thresholdMat && this.thresholdRT) {
      // Threshold pass: emissiveRT → thresholdRT (full resolution).
      this.thresholdMat.uniforms.tInput.value = this.sourceRT.texture;
      renderer.setRenderTarget(this.thresholdRT);
      renderer.clear();
      renderer.render(this.scene, this.camera);
      blurInput = this.thresholdRT;
    }

    // MipmapBlurPass reads blurInput.texture and writes to its own internal targets.
    this.mipmapBlur.render(renderer, blurInput, null, deltaTime);
  }

  override dispose(): void {
    this.thresholdRT?.dispose();
    this.thresholdMat?.dispose();
    this.mipmapBlur.dispose();
    super.dispose();
  }
}
