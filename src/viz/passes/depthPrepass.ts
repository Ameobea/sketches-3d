import { Pass, RenderPass } from 'postprocessing';

export class ClearDepthPass extends Pass {
  constructor() {
    super();
    this.needsSwap = false;
  }

  render(renderer: THREE.WebGLRenderer) {
    renderer.clearDepth();
  }
}

export class DepthPass extends RenderPass {
  constructor(scene: THREE.Scene, camera: THREE.Camera, overrideMaterial: THREE.Material) {
    super(scene, camera, overrideMaterial);
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number | undefined,
    stencilTest?: boolean | undefined
  ): void {
    renderer.shadowMap.enabled = false;
    // const dLight = this.scene.getObjectByName('pink_dlight') as THREE.DirectionalLight;
    // dLight.removeFromParent();
    renderer.getContext().depthFunc(renderer.getContext().LEQUAL);
    super.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);
    // this.scene.add(dLight);
    renderer.shadowMap.enabled = true;
  }
}

export class MainRenderPass extends RenderPass {
  constructor(scene: THREE.Scene, camera: THREE.Camera) {
    super(scene, camera);
    this.clear = false;
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number | undefined,
    stencilTest?: boolean | undefined
  ) {
    const ctx = renderer.getContext();
    ctx.depthFunc(ctx.EQUAL);
    super.render.apply(this, [renderer, inputBuffer, outputBuffer, deltaTime, stencilTest]);
    ctx.depthFunc(ctx.LEQUAL);
  }
}
