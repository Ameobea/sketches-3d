/**
 * Adapted from https://github.com/Ameobea/three-good-godrays/blob/main/src/compositorPass.ts
 */

import { CopyPass, Pass, type Resizable } from 'postprocessing';
import * as THREE from 'three';
import type { PerspectiveCamera } from 'three';

import VolumetricCompositorFragmentShader from './compositor.frag?raw';
import VolumetricCompositorVertexShader from './compositor.vert?raw';

export interface VolumetricPassCompositorParams {}

interface VolumetricCompositorMaterialProps {
  /**
   * Output of the volumetric pass
   */
  fogTexture: THREE.Texture;
  camera: THREE.PerspectiveCamera;
}

export class VolumetricCompositorMaterial extends THREE.ShaderMaterial implements Resizable {
  constructor({ fogTexture, camera }: VolumetricCompositorMaterialProps) {
    const uniforms = {
      fogTexture: { value: fogTexture },
      sceneDiffuse: { value: null },
      sceneDepth: { value: null },
      near: { value: 0.1 },
      far: { value: 1000.0 },
      resolution: { value: new THREE.Vector2(1, 1) },
      fogResolution: { value: new THREE.Vector2(1, 1) },
    };

    super({
      name: 'VolumetricCompositorMaterial',
      uniforms,
      depthWrite: false,
      depthTest: false,
      fragmentShader: VolumetricCompositorFragmentShader,
      vertexShader: VolumetricCompositorVertexShader,
      defines: {
        JBU_EXTENT: '1',
        JBU_SPATIAL_SIGMA: '1.0',
        JBU_DEPTH_SIGMA: '0.02',
      },
    });

    this.updateUniforms(camera.near, camera.far);
  }

  public updateUniforms(near: number, far: number): void {
    this.uniforms.near.value = near;
    this.uniforms.far.value = far;
  }

  setSize(width: number, height: number): void {
    this.uniforms.resolution.value.set(width, height);
  }

  setFogResolution(width: number, height: number): void {
    this.uniforms.fogResolution.value.set(width, height);
  }
}

export class VolumetricCompositorPass extends Pass {
  sceneCamera: PerspectiveCamera;
  private depthCopyRenderTexture: THREE.WebGLRenderTarget | null = null;
  private depthTextureCopyPass: CopyPass | null = null;

  constructor(props: VolumetricCompositorMaterialProps) {
    // version >= 6.38 of pmndrs/postprocessing uses an orthographic camera as default when
    // constructing a `Pass`.  The compositor code expects a default `THREE.Camera`.
    super('VolumetricCompositorPass', undefined, new THREE.Camera());
    this.fullscreenMaterial = new VolumetricCompositorMaterial(props);
    this.sceneCamera = props.camera;
  }

  public updateUniforms(): void {
    (this.fullscreenMaterial as VolumetricCompositorMaterial).updateUniforms(
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

  public setFogResolution(width: number, height: number): void {
    (this.fullscreenMaterial as VolumetricCompositorMaterial).setFogResolution(width, height);
  }
}
