import { Pass } from 'postprocessing';
import * as THREE from 'three';

import FogPassFragmentShader from './fogPassShader.frag?raw';
import FogPassVertexShader from './fogPassShader.vert?raw';

class FogPassMaterial extends THREE.ShaderMaterial {
  constructor(cameraNear: number, cameraFar: number) {
    super({
      name: 'FogPassMaterial',
      fragmentShader: FogPassFragmentShader,
      vertexShader: FogPassVertexShader,
      depthWrite: false,
      depthTest: false,
      uniforms: {
        sceneDistance: { value: null },
        sceneDiffuse: { value: null },
        cameraNear: { value: cameraNear },
        cameraFar: { value: cameraFar },
      },
    });
  }
}

export class FogPass extends Pass {
  getDistanceBuffer: () => THREE.WebGLRenderTarget;

  constructor(getDistanceBuffer: () => THREE.WebGLRenderTarget, camera: THREE.PerspectiveCamera) {
    super('FogPass');
    this.needsDepthTexture = false;
    this.fullscreenMaterial = new FogPassMaterial(camera.near, camera.far);
    this.getDistanceBuffer = getDistanceBuffer;
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget | null,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    (this.fullscreenMaterial as FogPassMaterial).uniforms.sceneDistance.value =
      this.getDistanceBuffer().texture;
    (this.fullscreenMaterial as FogPassMaterial).uniforms.sceneDiffuse.value = inputBuffer.texture;
    const oldDepthWrite = outputBuffer?.depthBuffer;
    if (outputBuffer) {
      outputBuffer.depthBuffer = false;
    }
    renderer.setRenderTarget(this.renderToScreen ? null : outputBuffer);
    renderer.render(this.scene, this.camera);
    if (outputBuffer) {
      outputBuffer.depthBuffer = oldDepthWrite!;
    }
  }
}
