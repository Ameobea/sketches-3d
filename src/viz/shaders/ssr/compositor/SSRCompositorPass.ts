import * as THREE from 'three';
import { Pass, type Resizable } from 'postprocessing';

import SSRCompositorShader from './compositor.frag?raw';
import SSRCompositorVertexShader from './compositor.vert?raw';

class SSRCompositorMaterial extends THREE.ShaderMaterial {
  constructor() {
    const uniforms = {
      ssrData: { value: null },
      sceneDiffuse: { value: null },
      sceneDepth: { value: null },
      sceneCameraPos: { value: new THREE.Vector3() },
      cameraNear: { value: 0 },
      cameraFar: { value: 0 },
      cameraProjectionMatrix: { value: new THREE.Matrix4() },
      cameraProjectionMatrixInv: { value: new THREE.Matrix4() },
      cameraMatrixWorld: { value: new THREE.Matrix4() },
      cameraMatrixWorldInverse: { value: new THREE.Matrix4() },
    };

    super({
      name: 'SSRCompositorMaterial',
      uniforms,
      depthWrite: false,
      depthTest: false,
      fragmentShader: SSRCompositorShader,
      vertexShader: SSRCompositorVertexShader,
    });
  }

  public updateUniforms(
    sceneCamera: THREE.PerspectiveCamera,
    inputBuffer: THREE.WebGLMultipleRenderTargets,
    outputBuffer: THREE.WebGLMultipleRenderTargets
  ) {
    this.uniforms.sceneCameraPos.value.copy(sceneCamera.position);
    this.uniforms.cameraNear.value = sceneCamera.near;
    this.uniforms.cameraFar.value = sceneCamera.far;
    this.uniforms.cameraProjectionMatrix.value.copy(sceneCamera.projectionMatrix);
    this.uniforms.cameraProjectionMatrixInv.value.copy(sceneCamera.projectionMatrixInverse);
    this.uniforms.cameraMatrixWorld.value.copy(sceneCamera.matrixWorld);
    this.uniforms.cameraMatrixWorldInverse.value.copy(sceneCamera.matrixWorldInverse);
    this.uniforms.sceneDiffuse.value = inputBuffer.texture[0];
    this.uniforms.ssrData.value = inputBuffer.texture[1];
    if (
      inputBuffer.depthTexture &&
      outputBuffer.depthTexture &&
      inputBuffer.depthTexture !== outputBuffer.depthTexture
    ) {
      throw new Error('Expected a single depth texture');
    }
    const depthTexture = inputBuffer.depthTexture || outputBuffer.depthTexture;
    if (!depthTexture) {
      console.error('No depth texture found for SSR compositor');
    }
    this.uniforms.sceneDepth.value = depthTexture;
  }
}

export class SSRCompositorPass extends Pass implements Resizable {
  private worldCamera: THREE.PerspectiveCamera;

  constructor(scene: THREE.Scene, worldCamera: THREE.PerspectiveCamera) {
    super('SSRCompositorPass', undefined, undefined);
    this.worldCamera = worldCamera;
    this.fullscreenMaterial = new SSRCompositorMaterial();
    this.needsDepthTexture = true;
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget | null,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    if (
      !(inputBuffer instanceof THREE.WebGLMultipleRenderTargets) ||
      !(outputBuffer instanceof THREE.WebGLMultipleRenderTargets)
    ) {
      console.error(
        'Expected multi render targets for SSR pass compositor, found: ',
        inputBuffer,
        outputBuffer
      );
      return;
    }

    (this.fullscreenMaterial as SSRCompositorMaterial).updateUniforms(
      this.worldCamera,
      inputBuffer,
      outputBuffer
    );
    renderer.setRenderTarget(this.renderToScreen ? null : outputBuffer);
    renderer.render(this.scene, this.camera);
  }

  setDepthTexture(
    depthTexture: THREE.Texture,
    depthPacking?: THREE.DepthPackingStrategies | undefined
  ): void {
    if (depthPacking && depthPacking !== THREE.BasicDepthPacking) {
      throw new Error('Only BasicDepthPacking is supported');
    }
    (this.fullscreenMaterial as SSRCompositorMaterial).uniforms.sceneDepth.value = depthTexture;
  }
}
