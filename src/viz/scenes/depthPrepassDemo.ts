import { EffectComposer, RenderPass } from 'postprocessing';
import * as THREE from 'three';

import type { SceneConfig } from '.';
import type { VizState } from '..';
import { loadTexture } from '../textureLoading';

const locations = {
  spawn: {
    pos: new THREE.Vector3(0, 5, 0),
    rot: new THREE.Vector3(0, 0, 0),
  },
};

const initScene = async (loadedWorld: THREE.Group) => {
  const loader = new THREE.ImageBitmapLoader();
  const cementTexture = await loadTexture(loader, 'https://ameo.link/u/amf.png');

  const ground = new THREE.Mesh(
    new THREE.BoxGeometry(100, 1, 100),
    new THREE.MeshStandardMaterial({ map: cementTexture })
  );
  ground.position.set(0, -1, 0);
  loadedWorld.add(ground);
  loadedWorld.add(new THREE.AmbientLight(new THREE.Color(0xcccccc)));
};

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
    renderer.getContext().depthFunc(renderer.getContext().LEQUAL);
    super.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);
  }
}

export class MainRenderPass extends RenderPass {
  constructor(scene: THREE.Scene, camera: THREE.Camera) {
    super(scene, camera);
    this.clear = false;
    this.clearPass.enabled = false;
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

export const processLoadedScene = async (viz: VizState, loadedWorld: THREE.Group): Promise<SceneConfig> => {
  await initScene(loadedWorld);

  /////////
  // Set up effect composer with depth pre-pass
  /////////

  // Auto-clearing the depth buffer must be disabled so that depth information from the pre-pass is preserved
  viz.renderer.autoClear = false;
  viz.renderer.autoClearDepth = false;

  const composer = new EffectComposer(viz.renderer);
  const depthPass = new DepthPass(viz.scene, viz.camera, new THREE.MeshBasicMaterial());
  // The depth pre pass must render to the same framebuffer as the main render pass so that the depth buffer is shared
  depthPass.renderToScreen = true;
  composer.addPass(depthPass);

  const mainRenderPass = new MainRenderPass(viz.scene, viz.camera);
  mainRenderPass.renderToScreen = true;
  composer.addPass(mainRenderPass);

  /////////
  // End effect composer setup
  /////////

  viz.setRenderOverride((timeDiffSeconds: number) => composer.render(timeDiffSeconds));

  viz.registerResizeCb(() => {
    composer.setSize(viz.renderer.domElement.width, viz.renderer.domElement.height);
  });

  return {
    locations,
    spawnLocation: 'spawn',
    player: {
      jumpVelocity: 0,
      enableDash: false,
      colliderCapsuleSize: {
        height: 1.35,
        radius: 0.3,
      },
      movementAccelPerSecond: {
        onGround: 3.6,
        inAir: 1,
      },
    },
    debugPos: true,
  };
};
