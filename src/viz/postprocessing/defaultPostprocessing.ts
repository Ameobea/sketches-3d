import {
  ClearMaskPass,
  type CopyPass,
  type Effect,
  EffectComposer,
  EffectPass,
  MaskPass,
  SMAAEffect,
  SMAAPreset,
  type Timer,
} from 'postprocessing';
import * as THREE from 'three';

import type { VizState } from 'src/viz';
import { GraphicsQuality } from 'src/viz/conf';
import { DepthPass, MainRenderPass } from 'src/viz/passes/depthPrepass';
import { CustomShaderMaterial } from '../shaders/customShader';
import { SSRCompositorPass } from '../shaders/ssr/compositor/SSRCompositorPass';

const populateShadowMap = (viz: VizState) => {
  const shadows: THREE.DirectionalLightShadow[] = [];
  viz.scene.traverse(obj => {
    if (obj instanceof THREE.DirectionalLight) {
      shadows.push(obj.shadow);
    }
  });

  // Render the scene once to populate the shadow map
  shadows.forEach(shadow => {
    shadow.needsUpdate = true;
  });
  viz.renderer.shadowMap.needsUpdate = true;
  viz.renderer.render(viz.scene, viz.camera);
  shadows.forEach(shadow => {
    shadow.needsUpdate = false;
    shadow.autoUpdate = false;
  });
  viz.renderer.shadowMap.needsUpdate = false;
  viz.renderer.shadowMap.autoUpdate = false;
  viz.renderer.shadowMap.enabled = true;
};

/**
 * Creates a render target (which maps to a framebuffer in WebGL) that contains two bound draw buffers.
 *
 * The first buffer/texture is shared with the default render target/framebuffer created by the the
 * effect composer.
 *
 * The second buffer is used to store data to facilitate screen-space reflections (SSR).
 */
const createSSRMultiFramebuffer = (
  renderer: THREE.WebGLRenderer,
  ssrDataTexture?: THREE.Texture
): THREE.WebGLMultipleRenderTargets => {
  const size = renderer === null ? new THREE.Vector2() : renderer.getDrawingBufferSize(new THREE.Vector2());
  const options = {
    minFilter: THREE.LinearFilter,
    magFilter: THREE.LinearFilter,
    stencilBuffer: false,
    depthBuffer: true,
    type: THREE.HalfFloatType,
  };
  const renderTarget = new THREE.WebGLMultipleRenderTargets(size.width, size.height, 2, options);
  // if (multisampling > 0) {
  //   renderTarget.ignoreDepthForMultisampleCopy = false;
  //   renderTarget.samples = multisampling;
  // }
  // if (type === UnsignedByteType2 && renderer !== null && renderer.outputColorSpace === SRGBColorSpace2) {
  //   renderTarget.texture.colorSpace = SRGBColorSpace2;
  // }
  renderTarget.texture[0].name = 'SHOUlD NOT BE USED; WILL BE REPLACED';
  renderTarget.texture[0].dispose();
  if (ssrDataTexture) {
    renderTarget.texture[1].name = 'OLD SSR BUFFER; WILL BE REPLACED';
    renderTarget.texture[1].dispose();
    renderTarget.texture[1] = ssrDataTexture;
  }
  renderTarget.texture[1].name = 'EffectComposer.ReflectionBuffer';
  renderTarget.texture[1].generateMipmaps = false;
  return renderTarget;
};

class CustomEffectComposer extends EffectComposer {
  public multiInputBuffer: THREE.WebGLMultipleRenderTargets;
  public multiOutputBuffer: THREE.WebGLMultipleRenderTargets;

  private didUseSSRBuffer = false;
  private ssrCompositorPass: SSRCompositorPass | null = null;

  /**
   * This monkey-patches Three.JS's renderer to work around issues when rendering materials that don't
   * make use of the SSR buffer (all vanilla materials and even the custom shader material if SSR isn't
   * explicitly enabled for the material).
   *
   * WebGL throws errors and renders nothing if a color attachment is bound and not written to.  To work
   * around this, this function checks if the material being rendered needs the SSR buffer or not.
   *
   * If it does, the draw buffer state is left as is (bound to the main color buffer and the SSR buffer).
   * If it doesn't, the second color attachment is set to `NONE` to avoid the error and then restored to
   * the original value after rendering.
   */
  private maybeHookRenderer(renderer: THREE.WebGLRenderer) {
    if ((renderer as any).isMultibufferHooked) {
      return;
    }

    // eslint-disable-next-line @typescript-eslint/no-this-alias
    const composer = this;
    const baseRenderBufferDirect = renderer.renderBufferDirect;
    const renderBufferDirect = function (
      camera: THREE.Camera,
      scene: THREE.Scene,
      geometry: THREE.BufferGeometry,
      material: THREE.Material,
      object: THREE.Object3D,
      group: any
    ) {
      const ctx = renderer.getContext() as WebGL2RenderingContext;
      const oldDrawBuffer0 = ctx.getParameter(ctx.DRAW_BUFFER0);
      const oldDrawBuffer1 = ctx.getParameter(ctx.DRAW_BUFFER1);

      const needsSSRBuffer = material instanceof CustomShaderMaterial && material.needsSSRBuffer;

      const newDrawBuffer0 = ctx.COLOR_ATTACHMENT0;
      const newDrawBuffer1 = needsSSRBuffer ? ctx.COLOR_ATTACHMENT1 : ctx.NONE;
      composer.didUseSSRBuffer ||= needsSSRBuffer;

      let didChangeDrawBuffers = false;
      if (
        oldDrawBuffer0 === ctx.COLOR_ATTACHMENT0 &&
        oldDrawBuffer1 !== null &&
        (oldDrawBuffer0 !== newDrawBuffer0 || oldDrawBuffer1 !== newDrawBuffer1)
      ) {
        didChangeDrawBuffers = true;
        ctx.drawBuffers([newDrawBuffer0, newDrawBuffer1]);
      }

      baseRenderBufferDirect.call(renderer, camera, scene, geometry, material, object, group);

      if (didChangeDrawBuffers) {
        ctx.drawBuffers([oldDrawBuffer0, oldDrawBuffer1]);
      }
    };

    (renderer as any).isMultibufferHooked = true;
    renderer.renderBufferDirect = renderBufferDirect;
  }

  constructor(
    private viz: VizState,
    options: {
      depthBuffer?: boolean;
      stencilBuffer?: boolean;
      alpha?: boolean;
      multisampling?: number;
      frameBufferType?: number;
    }
  ) {
    super(viz.renderer, options);

    this.multiInputBuffer = createSSRMultiFramebuffer(viz.renderer);
    this.multiOutputBuffer = createSSRMultiFramebuffer(viz.renderer, this.multiInputBuffer.texture[1]);

    this.maybeHookRenderer(viz.renderer);
  }

  /**
   * This is a copy of the base `EffectComposer.render` function with changes to facilitate SSR.
   */
  render(deltaTime: number) {
    const renderer = (this as any).renderer as THREE.WebGLRenderer;
    const copyPass = (this as any).copyPass as CopyPass;
    const timer = (this as any).timer as Timer;

    let inputBuffer = this.inputBuffer;
    let outputBuffer = this.outputBuffer;
    let multiInputBuffer = this.multiInputBuffer;
    let multiOutputBuffer = this.multiOutputBuffer;

    let stencilTest = false;
    let context, stencil, buffer, multiBuffer;

    if (deltaTime === undefined) {
      timer.update();
      deltaTime = timer.getDelta();
    }

    this.didUseSSRBuffer = false;
    for (let passIx = 0; passIx < this.passes.length; passIx++) {
      const pass = this.passes[passIx];
      if (pass instanceof SSRCompositorPass && !this.didUseSSRBuffer) {
        continue;
      }

      const isLastPass = passIx === this.passes.length - 1;
      if (isLastPass) {
        pass.renderToScreen = !this.didUseSSRBuffer;
      }

      if (pass.enabled) {
        const needsMultibuffer = pass instanceof MainRenderPass || pass instanceof SSRCompositorPass;
        if (needsMultibuffer) {
          multiInputBuffer.texture[0] = inputBuffer.texture;
          multiInputBuffer.depthTexture = inputBuffer.depthTexture;
          multiOutputBuffer.texture[0] = outputBuffer.texture;
          multiOutputBuffer.depthTexture = outputBuffer.depthTexture;
          pass.render(renderer, multiInputBuffer as any, multiOutputBuffer as any, deltaTime, stencilTest);
        } else {
          pass.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);
        }

        if (pass.needsSwap) {
          if (stencilTest) {
            throw new Error('Unimplemented');
            // copyPass.renderToScreen = pass.renderToScreen;
            // context = renderer.getContext();
            // stencil = renderer.state.buffers.stencil;

            // // Preserve the unaffected pixels.
            // stencil.setFunc(context.NOTEQUAL, 1, 0xffffffff);
            // copyPass.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);
            // stencil.setFunc(context.EQUAL, 1, 0xffffffff);
          }

          buffer = inputBuffer;
          inputBuffer = outputBuffer;
          outputBuffer = buffer;

          multiBuffer = multiInputBuffer;
          multiInputBuffer = multiOutputBuffer;
          multiOutputBuffer = multiBuffer;
        }

        if (pass instanceof MaskPass) {
          stencilTest = true;
        } else if (pass instanceof ClearMaskPass) {
          stencilTest = false;
        }
      }
    }

    if (this.didUseSSRBuffer) {
      if (!this.ssrCompositorPass) {
        this.ssrCompositorPass = new SSRCompositorPass(this.viz.scene, this.viz.camera);
        this.ssrCompositorPass.renderToScreen = true;
      }

      multiInputBuffer.texture[0] = inputBuffer.texture;
      multiInputBuffer.depthTexture = inputBuffer.depthTexture;
      multiOutputBuffer.texture[0] = outputBuffer.texture;
      multiOutputBuffer.depthTexture = outputBuffer.depthTexture;
      this.ssrCompositorPass.render(
        renderer,
        multiInputBuffer as any,
        multiOutputBuffer as any,
        deltaTime,
        stencilTest
      );
    }
  }
}

interface ExtraPostprocessingParams {
  toneMappingExposure?: number;
}

export const configureDefaultPostprocessingPipeline = (
  viz: VizState,
  quality: GraphicsQuality,
  addMiddlePasses?: (composer: EffectComposer, viz: VizState, quality: GraphicsQuality) => void,
  onFirstRender?: () => void,
  extraParams: Partial<ExtraPostprocessingParams> = {},
  postEffects?: Effect[]
) => {
  const effectComposer = new CustomEffectComposer(viz, {
    multisampling: 0,
    frameBufferType: THREE.HalfFloatType,
  });

  viz.renderer.autoClear = false;
  viz.renderer.autoClearColor = true;
  viz.renderer.autoClearDepth = false;
  const depthPrePassMaterial = new THREE.MeshBasicMaterial();
  const depthPass = new DepthPass(viz.scene, viz.camera, depthPrePassMaterial);
  depthPass.skipShadowMapUpdate = true;
  effectComposer.addPass(depthPass);

  const renderPass = new MainRenderPass(viz.scene, viz.camera);
  renderPass.skipShadowMapUpdate = true;
  renderPass.needsDepthTexture = true;
  effectComposer.addPass(renderPass);

  addMiddlePasses?.(effectComposer, viz, quality);

  const smaaEffect = new SMAAEffect({
    preset: {
      [GraphicsQuality.Low]: SMAAPreset.LOW,
      [GraphicsQuality.Medium]: SMAAPreset.MEDIUM,
      [GraphicsQuality.High]: SMAAPreset.HIGH,
    }[quality],
  });
  const fxPass = new EffectPass(viz.camera, ...[smaaEffect, ...(postEffects ?? [])]);
  effectComposer.addPass(fxPass);

  viz.renderer.toneMapping = THREE.ACESFilmicToneMapping;
  if (extraParams.toneMappingExposure) {
    viz.renderer.toneMappingExposure = extraParams.toneMappingExposure;
  }

  let didRender = false;
  viz.setRenderOverride(timeDiffSeconds => {
    effectComposer.render(timeDiffSeconds);
    viz.renderer.shadowMap.autoUpdate = false;
    viz.renderer.shadowMap.needsUpdate = false;

    // For some reason, the shadow map that we render at the start of everything is getting cleared at some
    // point during the setup of this postprocessing pipeline.
    //
    // So, we have to re-populate the shadowmap so that it can be used to power the godrays and, well, shadows.
    if (!didRender && viz.renderer.shadowMap.enabled) {
      didRender = true;
      populateShadowMap(viz);
    }
  });
};
