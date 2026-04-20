import { Pass } from 'postprocessing';
import * as THREE from 'three';
import FRAGMENT_SHADER from './shaders/final.frag?raw';
import VERTEX_SHADER from './shaders/final.vert?raw';
import type { Viz } from '..';

export type ToneMappingMode = 'none' | 'aces' | 'cineon' | 'reinhard' | 'agx' | 'neutral';

class FinalPassMaterial extends THREE.ShaderMaterial {
  constructor(
    toneMapping: ToneMappingMode,
    exposure: number,
    emissiveBuffer: THREE.Texture | null,
    emissiveBloomBuffer: THREE.Texture | null,
    bloomIntensity: number,
    fogShader: string | undefined,
    skyBypassTonemap: boolean
  ) {
    const defines: Record<string, string> = {};
    if (toneMapping === 'aces') defines.TONE_MAPPING_ACES = '1';
    else if (toneMapping === 'cineon') defines.TONE_MAPPING_CINEON = '1';
    else if (toneMapping === 'reinhard') defines.TONE_MAPPING_REINHARD = '1';
    else if (toneMapping === 'agx') defines.TONE_MAPPING_AGX = '1';
    else if (toneMapping === 'neutral') defines.TONE_MAPPING_NEUTRAL = '1';

    if (emissiveBuffer !== null) defines.HAS_EMISSIVE_BUFFER = '1';
    if (emissiveBloomBuffer !== null) defines.HAS_EMISSIVE_BLOOM = '1';
    if (fogShader) defines.HAS_FOG = '1';
    if (skyBypassTonemap) defines.SKY_BYPASS_TONEMAP = '1';

    const needsDepth = !!fogShader || skyBypassTonemap;

    const uniforms: Record<string, THREE.IUniform> = {
      inputBuffer: { value: null },
      toneMappingExposure: { value: exposure },
      gammaExponent: { value: 1.0 },
    };
    if (emissiveBuffer !== null) {
      uniforms.emissiveBuffer = { value: emissiveBuffer };
    }
    if (emissiveBloomBuffer !== null) {
      uniforms.emissiveBloomBuffer = { value: emissiveBloomBuffer };
      uniforms.bloomIntensity = { value: bloomIntensity };
    }
    if (needsDepth) {
      uniforms.depthBuffer = { value: null };
    }
    if (fogShader) {
      // These two are set to the camera's own matrix objects so they stay in sync
      // without any per-frame copy — Three.js reads uniform.value by reference.
      uniforms.projectionMatrixInverse = { value: new THREE.Matrix4() };
      uniforms.cameraWorldMatrix = { value: new THREE.Matrix4() };
      uniforms.fogCameraPos = { value: new THREE.Vector3() };
      uniforms.fogPlayerPos = { value: new THREE.Vector3() };
      uniforms.curTimeSeconds = { value: 0.0 };
    }

    super({
      name: 'FinalPassMaterial',
      uniforms,
      defines,
      vertexShader: VERTEX_SHADER,
      // Prepend the user's fog function so it's available to the #ifdef HAS_FOG call in main().
      fragmentShader: fogShader ? fogShader + '\n' + FRAGMENT_SHADER : FRAGMENT_SHADER,
      depthWrite: false,
      depthTest: false,
    });
  }
}

export class FinalPass extends Pass {
  private readonly viz: Viz;
  private readonly mat: FinalPassMaterial;
  private readonly hasFogShader: boolean;
  private readonly needsDepth: boolean;
  private storedDepthTexture: THREE.Texture | null = null;

  override setDepthTexture(depthTexture: THREE.Texture | null): void {
    this.storedDepthTexture = depthTexture;
  }

  public setBloomIntensity(value: number): void {
    if (this.mat.uniforms.bloomIntensity) {
      this.mat.uniforms.bloomIntensity.value = value;
    }
  }

  /** gamma=1.0 is identity; >1.0 brightens midtones, <1.0 darkens. */
  public setGamma(gamma: number): void {
    this.mat.uniforms.gammaExponent.value = 1.0 / gamma;
  }

  constructor(
    viz: Viz,
    {
      toneMapping = 'aces',
      exposure = 1.0,
      emissiveBuffer = null,
      emissiveBloomBuffer = null,
      bloomIntensity = 1.0,
      fogShader,
      skyBypassTonemap = false,
    }: {
      toneMapping?: ToneMappingMode;
      exposure?: number;
      emissiveBuffer?: THREE.Texture | null;
      emissiveBloomBuffer?: THREE.Texture | null;
      bloomIntensity?: number;
      /**
       * GLSL string that defines the fog function. Will be prepended to the final pass fragment
       * shader. Must define:
       *
       *   vec4 getFogEffect(vec3 worldPos, vec3 cameraPos, vec3 playerPos,
       *                     float depth, float curTimeSeconds)
       *
       * Returns vec4(fogColor.rgb, fogFactor) where fogFactor=0 is clear, 1 is full fog.
       * `depth` is the raw depth buffer value in [0,1]; depth >= ~0.9999 means sky / no geometry.
       * Uses GLSL ES 1.00 style (texture2D, etc.) since the final pass does not use GLSL3.
       */
      fogShader?: string;
      /**
       * When true, fragments at the depth-buffer far plane (sky / no geometry) skip tone
       * mapping and the exposure scale, going straight to sRGB encoding. Lets sky shaders
       * author display-referred colors that are preserved 1:1 through the pipeline. sRGB
       * encoding, gamma, dither, emissive composite, and bloom still run normally.
       */
      skyBypassTonemap?: boolean;
    } = {}
  ) {
    super('FinalPass', undefined, new THREE.Camera());
    this.viz = viz;
    this.mat = new FinalPassMaterial(
      toneMapping,
      exposure,
      emissiveBuffer,
      emissiveBloomBuffer,
      bloomIntensity,
      fogShader,
      skyBypassTonemap
    );
    this.hasFogShader = !!fogShader;
    this.needsDepth = !!fogShader || skyBypassTonemap;
    this.fullscreenMaterial = this.mat;

    if (this.needsDepth) {
      this.needsDepthTexture = true;
    }

    if (fogShader) {
      this.mat.uniforms.projectionMatrixInverse.value = this.viz.camera.projectionMatrixInverse;
      this.mat.uniforms.cameraWorldMatrix.value = this.viz.camera.matrixWorld;

      this.mat.uniforms.fogPlayerPos.value = new THREE.Vector3();
    }
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    _deltaTime?: number,
    _stencilTest?: boolean
  ): void {
    this.mat.uniforms.inputBuffer.value = inputBuffer.texture;

    if (this.mat.uniforms.depthBuffer) {
      this.mat.uniforms.depthBuffer.value = this.storedDepthTexture;
    }
    if (this.hasFogShader) {
      this.mat.uniforms.fogCameraPos.value.setFromMatrixPosition(this.viz.camera.matrixWorld);
    }

    if (this.hasFogShader && this.viz.fpCtx) {
      this.mat.uniforms.curTimeSeconds.value = this.viz.fpCtx.getPhysicsTime();
      const playerPos = this.viz.fpCtx.playerController.getPosition();
      if (playerPos) {
        this.mat.uniforms.fogPlayerPos.value.set(playerPos.x(), playerPos.y(), playerPos.z());
      }
    }

    renderer.setRenderTarget(this.renderToScreen ? null : outputBuffer);
    renderer.render(this.scene, this.camera);
  }
}
