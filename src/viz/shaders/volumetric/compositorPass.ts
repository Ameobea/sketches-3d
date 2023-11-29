/**
 * Adapted from https://github.com/Ameobea/three-good-godrays/blob/main/src/compositorPass.ts
 */

import { CopyPass, Pass, Resizable } from 'postprocessing';
import * as THREE from 'three';
import type { PerspectiveCamera } from 'three';

import VolumetricCompositorFragmentShader from './compositor.frag?raw';
import VolumetricCompositorVertexShader from './compositor.vert?raw';

export interface VolumetricPassCompositorParams {
  edgeStrength: number;
  edgeRadius: number;
}

const DefaultVolumetricPassCompositorParams: VolumetricPassCompositorParams = Object.freeze({
  edgeStrength: 2,
  edgeRadius: 2,
});

interface VolumetricCompositorMaterialProps {
  /**
   * Output of the volumetric pass
   */
  fogTexture: THREE.Texture;
  params: Partial<VolumetricPassCompositorParams> | undefined;
  camera: THREE.PerspectiveCamera;
}

export class VolumetricCompositorMaterial extends THREE.ShaderMaterial implements Resizable {
  constructor({ fogTexture, params, camera }: VolumetricCompositorMaterialProps) {
    const uniforms = {
      fogTexture: { value: fogTexture },
      sceneDiffuse: { value: null },
      sceneDepth: { value: null },
      edgeStrength: { value: params?.edgeStrength ?? DefaultVolumetricPassCompositorParams.edgeStrength },
      edgeRadius: { value: params?.edgeRadius ?? DefaultVolumetricPassCompositorParams.edgeRadius },
      near: { value: 0.1 },
      far: { value: 1000.0 },
      resolution: { value: new THREE.Vector2(1, 1) },
    };

    super({
      name: 'VolumetricCompositorMaterial',
      uniforms,
      depthWrite: false,
      depthTest: false,
      fragmentShader: VolumetricCompositorFragmentShader,
      vertexShader: VolumetricCompositorVertexShader,
    });

    this.updateUniforms(params, camera.near, camera.far);
  }

  public updateUniforms(
    params: Partial<VolumetricPassCompositorParams> | undefined,
    near: number,
    far: number
  ): void {
    this.uniforms.edgeStrength.value =
      params?.edgeStrength ?? DefaultVolumetricPassCompositorParams.edgeStrength;
    this.uniforms.edgeRadius.value = params?.edgeRadius ?? DefaultVolumetricPassCompositorParams.edgeRadius;
    this.uniforms.near.value = near;
    this.uniforms.far.value = far;
  }

  setSize(width: number, height: number): void {
    this.uniforms.resolution.value.set(width, height);
  }
}

export class VolumetricCompositorPass extends Pass {
  sceneCamera: PerspectiveCamera;
  private depthCopyRenderTexture: THREE.WebGLRenderTarget | null = null;
  private depthTextureCopyPass: CopyPass | null = null;

  constructor(props: VolumetricCompositorMaterialProps) {
    super('VolumetricCompositorPass');
    this.fullscreenMaterial = new VolumetricCompositorMaterial(props);
    this.sceneCamera = props.camera;
  }

  public updateUniforms(
    params: Partial<VolumetricPassCompositorParams> = DefaultVolumetricPassCompositorParams
  ): void {
    (this.fullscreenMaterial as VolumetricCompositorMaterial).updateUniforms(
      params,
      this.sceneCamera.near,
      this.sceneCamera.far
    );
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget | null,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    (this.fullscreenMaterial as VolumetricCompositorMaterial).uniforms.sceneDiffuse.value =
      inputBuffer.texture;

    // There is a limitation in the pmndrs postprocessing library that causes rendering issues when
    // the depth texture provided to the effect is the same as the one bound to the output buffer.
    //
    // To work around this, we copy the depth texture to a new render target and use that instead
    // if it's found to be the same.
    const sceneDepth = (this.fullscreenMaterial as VolumetricCompositorMaterial).uniforms.sceneDepth.value;
    if (sceneDepth && outputBuffer && sceneDepth === outputBuffer.depthTexture) {
      if (!this.depthCopyRenderTexture) {
        this.depthCopyRenderTexture = new THREE.WebGLRenderTarget(
          outputBuffer.depthTexture.image.width,
          outputBuffer.depthTexture.image.height,
          {
            minFilter: outputBuffer.depthTexture.minFilter,
            magFilter: outputBuffer.depthTexture.magFilter,
            format: outputBuffer.depthTexture.format,
            generateMipmaps: outputBuffer.depthTexture.generateMipmaps,
          }
        );
      }
      if (!this.depthTextureCopyPass) {
        this.depthTextureCopyPass = new CopyPass();
      }

      this.depthTextureCopyPass.render(
        renderer,
        (this.fullscreenMaterial as VolumetricCompositorMaterial).uniforms.sceneDepth.value,
        this.depthCopyRenderTexture
      );
      (this.fullscreenMaterial as VolumetricCompositorMaterial).uniforms.sceneDepth.value =
        this.depthCopyRenderTexture.texture;
    }

    renderer.setRenderTarget(outputBuffer);
    renderer.render(this.scene, this.camera);

    (this.fullscreenMaterial as VolumetricCompositorMaterial).uniforms.sceneDepth.value = sceneDepth;
  }

  override setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    if (depthPacking && depthPacking !== THREE.BasicDepthPacking) {
      throw new Error('Only BasicDepthPacking is supported');
    }
    (this.fullscreenMaterial as VolumetricCompositorMaterial).uniforms.sceneDepth.value = depthTexture;
  }

  override setSize(width: number, height: number): void {
    (this.fullscreenMaterial as VolumetricCompositorMaterial).setSize(width, height);
  }
}
