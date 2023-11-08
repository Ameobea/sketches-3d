import { Pass, RenderPass, type Resizable, Selection } from 'postprocessing';
import * as THREE from 'three';

export class ClearDepthPass extends Pass {
  constructor() {
    super();
    this.needsSwap = false;
  }

  render(renderer: THREE.WebGLRenderer) {
    renderer.clearDepth();
  }
}

export class DepthPass extends RenderPass implements Resizable {
  public renderTarget: THREE.WebGLRenderTarget | null = null;

  constructor(
    scene: THREE.Scene,
    camera: THREE.Camera,
    overrideMaterial: THREE.Material,
    useExternalRenderTarget?: boolean
  ) {
    super(scene, camera, overrideMaterial);
    if (useExternalRenderTarget) {
      this.renderTarget = new THREE.WebGLRenderTarget(1, 1, {
        format: THREE.RGBAFormat,
        type: THREE.UnsignedByteType,
        depthBuffer: true,
      });
    }
  }

  render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    outputBuffer: THREE.WebGLRenderTarget,
    deltaTime?: number | undefined,
    stencilTest?: boolean | undefined
  ): void {
    // Avoid rendering transparent objects
    const selection: THREE.Object3D<THREE.Event>[] = [];
    this.scene.traverse(c => {
      if (c instanceof THREE.Mesh && c.material.transparent) {
        return;
      }

      selection.push(c);
    });
    if (!this.selection) {
      this.selection = new Selection(selection);
    }
    this.selection.clear();
    for (const child of selection) {
      this.selection.add(child);
    }

    const shadowMapWasEnabled = renderer.shadowMap.enabled;
    renderer.shadowMap.enabled = false;
    renderer.getContext().depthFunc(renderer.getContext().LEQUAL);
    if (this.renderTarget) {
      // I have no idea why, but `RenderPass` seems to render to `inputBuffer` instead of `outputBuffer`
      this.renderTarget.depthTexture = inputBuffer.depthTexture;
      super.render(renderer, this.renderTarget, outputBuffer, deltaTime, stencilTest);
    } else {
      super.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);
    }

    renderer.shadowMap.enabled = shadowMapWasEnabled;
  }

  setSize(width: number, height: number): void {
    this.renderTarget?.setSize(width, height);
    super.setSize(width, height);
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
    // ctx.depthFunc(ctx.EQUAL);
    super.render.apply(this, [renderer, inputBuffer, outputBuffer, deltaTime, stencilTest]);
    // ctx.depthFunc(ctx.LEQUAL);
  }
}
