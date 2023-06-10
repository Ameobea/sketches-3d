import { Pass } from 'postprocessing';
import { ShaderMaterial, type WebGLRenderTarget } from 'three';

import FogPassFragmentShader from './fogPassShader.frag?raw';
import FogPassVertexShader from './fogPassShader.vert?raw';

// import FogFragmentShader from './fogShader.frag?raw';

// export class FogEffect extends Effect {
//   private getDistanceBuffer: () => WebGLRenderTarget;

//   constructor(getDistanceBuffer: () => WebGLRenderTarget, blendFunction?: BlendFunction) {
//     const uniforms = new Map();
//     uniforms.set('sceneDistance', new Uniform(null));
//     super('FogShader', FogFragmentShader, { uniforms, blendFunction });
//     this.getDistanceBuffer = getDistanceBuffer;
//   }

//   update(renderer: WebGLRenderer, inputBuffer: WebGLRenderTarget, deltaTime?: number | undefined): void {
//     this.uniforms.get('sceneDistance')!.value = this.getDistanceBuffer().texture;
//   }
// }

class FogPassMaterial extends ShaderMaterial {
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
  getDistanceBuffer: () => WebGLRenderTarget;

  constructor(getDistanceBuffer: () => WebGLRenderTarget, camera: THREE.PerspectiveCamera) {
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
    renderer.setRenderTarget(outputBuffer);
    renderer.render(this.scene, this.camera);
  }
}
