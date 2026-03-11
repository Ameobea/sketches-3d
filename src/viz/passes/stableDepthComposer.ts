/**
 * StableDepthEffectComposer — a drop-in subclass of EffectComposer that keeps the depth
 * texture on a separate, immutable render target that is never part of the ping-pong rotation.
 *
 * Problem this solves:
 *   EffectComposer attaches the depth texture to `inputBuffer`. After any pass with
 *   `needsSwap = true` runs (e.g. N8AOPostPass), inputBuffer and outputBuffer swap. The depth
 *   texture ends up on what is now `outputBuffer`. Any subsequent pass that both reads from
 *   that depth texture AND renders into `outputBuffer` creates a WebGL feedback loop, causing
 *   artifacts or missing output.
 *
 * Solution:
 *   Override render() to inject a single blitFramebuffer call immediately before the first
 *   swap fires each frame. At that point inputBuffer still holds the scene's fresh depth (the
 *   render passes run before any swap). The blit copies depth into a dedicated stableDepthTarget
 *   that is never used as a render output. All passes receive stableDepthTarget.depthTexture via
 *   setDepthTexture, so they always read from a texture that cannot be simultaneously bound as a
 *   framebuffer attachment — no feedback loop possible.
 *
 * Usage: replace `new EffectComposer(renderer, opts)` with
 *        `new StableDepthEffectComposer(renderer, opts)`.
 *        No other changes required in the rest of the pipeline.
 */

import { ClearMaskPass, EffectComposer, MaskPass, Pass } from 'postprocessing';
import * as THREE from 'three';

class DepthCopyToStable {
  readonly stableDepthTarget: THREE.WebGLRenderTarget;

  constructor(width: number, height: number) {
    const dt = new THREE.DepthTexture(width, height);
    // Match EffectComposer.createDepthTexture() which uses UnsignedIntType (= DEPTH_COMPONENT24).
    // If you construct EffectComposer with { stencilBuffer: true }, change to UnsignedInt248Type.
    dt.type = THREE.UnsignedIntType;
    this.stableDepthTarget = new THREE.WebGLRenderTarget(width, height, {
      depthBuffer: true,
      depthTexture: dt,
    });
  }

  blit(renderer: THREE.WebGLRenderer, inputBuffer: THREE.WebGLRenderTarget): void {
    const gl = renderer.getContext() as WebGL2RenderingContext;
    const props = (renderer as any).properties;

    // setRenderTarget lazily creates the FBO for stableDepthTarget (first call and after resize).
    renderer.setRenderTarget(this.stableDepthTarget);

    const srcFBO: WebGLFramebuffer = props.get(inputBuffer).__webglFramebuffer;
    const dstFBO: WebGLFramebuffer = props.get(this.stableDepthTarget).__webglFramebuffer;
    const { width, height } = inputBuffer;

    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, srcFBO);
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, dstFBO);
    gl.blitFramebuffer(0, 0, width, height, 0, 0, width, height, gl.DEPTH_BUFFER_BIT, gl.NEAREST);

    // Restore GL state so three.js's internal tracking stays consistent.
    gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
    gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
    renderer.setRenderTarget(null);
  }

  setSize(width: number, height: number): void {
    this.stableDepthTarget.setSize(width, height);
  }

  dispose(): void {
    this.stableDepthTarget.dispose();
  }
}

export class StableDepthEffectComposer extends EffectComposer {
  private stableDepth: DepthCopyToStable | null = null;

  override addPass(pass: Pass, index?: number): void {
    const hadDepth = !!(this as any).depthTexture;
    super.addPass(pass, index);
    const hasDepth = !!(this as any).depthTexture;

    if (!hadDepth && hasDepth) {
      // Depth texture was just created for the first time. Create the stable target.
      const { width, height } = (this as any).inputBuffer as THREE.WebGLRenderTarget;
      this.stableDepth = new DepthCopyToStable(width, height);

      // Redirect every existing pass to use the stable depth texture instead of the
      // ping-pong buffer's depth texture.
      const stableDT = this.stableDepth.stableDepthTarget.depthTexture!;
      for (const p of (this as any).passes as Pass[]) {
        p.setDepthTexture(stableDT);
      }
    } else if (this.stableDepth) {
      // Stable depth already established — give it to this newly-added pass.
      pass.setDepthTexture(this.stableDepth.stableDepthTarget.depthTexture!);
    }
  }

  override render(deltaTime?: number): void {
    const renderer = this.renderer;
    const stableDepth = this.stableDepth;

    if (!renderer || !stableDepth) {
      super.render(deltaTime);
      return;
    }

    // Replicate EffectComposer's render loop with one addition: blit depth to the stable
    // target immediately before the first pass that swaps buffers. At that moment inputBuffer
    // still contains the scene's current-frame depth written by the render passes above.
    if (deltaTime === undefined) {
      (this as any).timer.update();
      deltaTime = (this as any).timer.getDelta();
    }

    const passes = (this as any).passes as Pass[];
    let inputBuffer = (this as any).inputBuffer as THREE.WebGLRenderTarget;
    let outputBuffer = (this as any).outputBuffer as THREE.WebGLRenderTarget;
    const copyPass = (this as any).copyPass as Pass;

    let depthBlitted = false;
    let stencilTest = false;

    for (const pass of passes) {
      if (!pass.enabled) continue;

      // Blit depth right before the first swap. inputBuffer.depthTexture being non-null
      // confirms the scene has rendered and depth is available on this buffer.
      if (!depthBlitted && pass.needsSwap && inputBuffer.depthTexture) {
        stableDepth.blit(renderer, inputBuffer);
        depthBlitted = true;
      }

      pass.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);

      if (pass.needsSwap) {
        if (stencilTest) {
          const prevRTS = copyPass.renderToScreen;
          copyPass.renderToScreen = pass.renderToScreen;
          const gl = renderer.getContext();
          const stencilBuf = (renderer as any).state.buffers.stencil;
          stencilBuf.setFunc(gl.NOTEQUAL, 1, 0xffffffff);
          copyPass.render(renderer, inputBuffer, outputBuffer, deltaTime, stencilTest);
          stencilBuf.setFunc(gl.EQUAL, 1, 0xffffffff);
          copyPass.renderToScreen = prevRTS;
        }
        const buf = inputBuffer;
        inputBuffer = outputBuffer;
        outputBuffer = buf;
      }

      if (pass instanceof MaskPass) {
        stencilTest = true;
      } else if (pass instanceof ClearMaskPass) {
        stencilTest = false;
      }
    }
  }

  override setSize(width: number, height: number): void {
    super.setSize(width, height);
    this.stableDepth?.setSize(width, height);
  }

  override dispose(): void {
    this.stableDepth?.dispose();
    super.dispose();
  }
}
