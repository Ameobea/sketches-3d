/**
 * Adapted from https://github.com/Ameobea/three-good-godrays/blob/main/src/compositorPass.ts
 */

import { CopyPass, Pass, type Resizable } from 'postprocessing';
import * as THREE from 'three';
import type { PerspectiveCamera } from 'three';

import VolumetricCompositorFragmentShader from './compositor.frag?raw';
import VolumetricCompositorVertexShader from './compositor.vert?raw';

interface VolumetricCompositorMaterialProps {
  /**
   * Output of the volumetric pass (rgb = fog color, a = density)
   */
  fogTexture: THREE.Texture;
  /**
   * Raw scene depth captured by the volumetric pass at half resolution.
   * Used by the JBU instead of re-sampling the full-res depth buffer, ensuring
   * the depth reference for each low-res texel matches exactly what the volumetric
   * pass saw when it raymarched that pixel.
   */
  fogDepthTexture: THREE.Texture;
  camera: THREE.PerspectiveCamera;
  /** See `VolumetricPassParams.jbuExtent`. Default: 1 */
  jbuExtent?: number;
  /** See `VolumetricPassParams.jbuSpatialSigma`. Default: 1.8 */
  jbuSpatialSigma?: number;
  /** See `VolumetricPassParams.jbuDepthSigma`. Default: 0.034 */
  jbuDepthSigma?: number;
}

export class VolumetricCompositorMaterial extends THREE.ShaderMaterial implements Resizable {
  constructor({
    fogTexture,
    fogDepthTexture,
    camera,
    jbuExtent,
    jbuSpatialSigma,
    jbuDepthSigma,
  }: VolumetricCompositorMaterialProps) {
    const uniforms = {
      fogTexture: { value: fogTexture },
      fogDepthTexture: { value: fogDepthTexture },
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
        JBU_EXTENT: String(jbuExtent ?? 1),
        JBU_SPATIAL_SIGMA: String(jbuSpatialSigma ?? 1.8),
        JBU_DEPTH_SIGMA: String(jbuDepthSigma ?? 0.034),
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
        const dt = outputBuffer.depthTexture!;
        this.depthCopyRenderTexture = new THREE.WebGLRenderTarget(dt.image.width, dt.image.height, {
          minFilter: dt.minFilter,
          magFilter: dt.magFilter,
          format: dt.format,
          generateMipmaps: dt.generateMipmaps,
        });
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
