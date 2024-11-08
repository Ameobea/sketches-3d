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
}

export class SSRCompositorPass extends Pass implements Resizable {
  constructor() {
    super('SSRCompositorPass');
    this.fullscreenMaterial = new SSRCompositorMaterial();
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget | null,
    _deltaTime?: number | undefined,
    _stencilTest?: boolean | undefined
  ): void {
    if (!(inputBuffer instanceof THREE.WebGLMultipleRenderTargets)) {
      console.error(
        'Expected multi render target for SSR pass compositor, found: ',
        inputBuffer,
        outputBuffer
      );
      return;
    }

    (this.fullscreenMaterial as SSRCompositorMaterial).uniforms.sceneDiffuse.value = inputBuffer.texture[0];
    (this.fullscreenMaterial as SSRCompositorMaterial).uniforms.ssrData.value = inputBuffer.texture[1];
    (this.fullscreenMaterial as SSRCompositorMaterial).uniforms.sceneDepth.value = inputBuffer.depthTexture;
    renderer.setRenderTarget(this.renderToScreen ? null : outputBuffer);
    renderer.render(this.scene, this.camera);
  }
}
