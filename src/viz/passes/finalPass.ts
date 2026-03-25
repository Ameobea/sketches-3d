import { Pass } from 'postprocessing';
import * as THREE from 'three';
import FRAGMENT_SHADER from './shaders/final.frag?raw';
import VERTEX_SHADER from './shaders/final.vert?raw';

export type ToneMappingMode = 'none' | 'aces' | 'cineon' | 'reinhard' | 'agx' | 'neutral';

class FinalPassMaterial extends THREE.ShaderMaterial {
  constructor(
    toneMapping: ToneMappingMode,
    exposure: number,
    emissiveBuffer: THREE.Texture | null,
    emissiveBloomBuffer: THREE.Texture | null,
    bloomIntensity: number
  ) {
    const defines: Record<string, string> = {};
    if (toneMapping === 'aces') defines.TONE_MAPPING_ACES = '1';
    else if (toneMapping === 'cineon') defines.TONE_MAPPING_CINEON = '1';
    else if (toneMapping === 'reinhard') defines.TONE_MAPPING_REINHARD = '1';
    else if (toneMapping === 'agx') defines.TONE_MAPPING_AGX = '1';
    else if (toneMapping === 'neutral') defines.TONE_MAPPING_NEUTRAL = '1';

    if (emissiveBuffer !== null) defines.HAS_EMISSIVE_BUFFER = '1';
    if (emissiveBloomBuffer !== null) defines.HAS_EMISSIVE_BLOOM = '1';

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

    super({
      name: 'FinalPassMaterial',
      uniforms,
      defines,
      vertexShader: VERTEX_SHADER,
      fragmentShader: FRAGMENT_SHADER,
      depthWrite: false,
      depthTest: false,
    });
  }
}

export class FinalPass extends Pass {
  private readonly mat: FinalPassMaterial;

  public setBloomIntensity(value: number): void {
    if (this.mat.uniforms.bloomIntensity) {
      this.mat.uniforms.bloomIntensity.value = value;
    }
  }

  /** gamma=1.0 is identity; >1.0 brightens midtones, <1.0 darkens. */
  public setGamma(gamma: number): void {
    this.mat.uniforms.gammaExponent.value = 1.0 / gamma;
  }

  constructor({
    toneMapping = 'aces',
    exposure = 1.0,
    emissiveBuffer = null,
    emissiveBloomBuffer = null,
    bloomIntensity = 1.0,
  }: {
    toneMapping?: ToneMappingMode;
    exposure?: number;
    emissiveBuffer?: THREE.Texture | null;
    emissiveBloomBuffer?: THREE.Texture | null;
    bloomIntensity?: number;
  } = {}) {
    super('FinalPass', undefined, new THREE.Camera());
    this.mat = new FinalPassMaterial(
      toneMapping,
      exposure,
      emissiveBuffer,
      emissiveBloomBuffer,
      bloomIntensity
    );
    this.fullscreenMaterial = this.mat;
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    _deltaTime?: number,
    _stencilTest?: boolean
  ): void {
    this.mat.uniforms.inputBuffer.value = inputBuffer.texture;
    renderer.setRenderTarget(this.renderToScreen ? null : outputBuffer);
    renderer.render(this.scene, this.camera);
  }
}
