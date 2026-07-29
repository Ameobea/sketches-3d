/**
 * StableDepthEffectComposer — EffectComposer subclass that fixes the stable-depth texture
 * aliasing bug in postprocessing >= 6.39 and exposes the stable depth target.
 *
 * postprocessing 6.39 moved stable-depth handling upstream: a dedicated `depthRenderTarget`
 * that is never rendered into, populated via blitFramebuffer after every RenderPass, whose
 * depthTexture is what all passes sample (preventing depth-read-while-attached feedback
 * loops after ping-pong swaps). But its `createDepthTexture` builds the output/stable depth
 * textures with `DepthTexture.clone()`. Cloned textures share their `Source`, and three
 * allocates GL textures per (source, cacheKey), so all three "separate" depth textures
 * alias one GL image. The per-frame depth blit then has the same image bound to READ and
 * DRAW — GL_INVALID_OPERATION on ANGLE — and the feedback-loop protection is silently
 * defeated. Shadowing `createDepthTexture` with independently-constructed textures restores
 * the intended separation.
 */

import { EffectComposer } from 'postprocessing';
import * as THREE from 'three';

export class StableDepthEffectComposer extends EffectComposer {
  get stableDepthTarget(): THREE.WebGLRenderTarget | null {
    return (this as any).depthRenderTarget ?? null;
  }

  /** Shadows EffectComposer's private method of the same name (called from `addPass`). */
  protected createDepthTexture(): void {
    const inputBuffer = (this as any).inputBuffer as THREE.WebGLRenderTarget;
    const outputBuffer = (this as any).outputBuffer as THREE.WebGLRenderTarget;
    const { width, height } = inputBuffer;

    const mkDepthTexture = (name: string) => {
      const dt = new THREE.DepthTexture(width, height);
      if (inputBuffer.stencilBuffer) {
        dt.format = THREE.DepthStencilFormat;
        dt.type = THREE.UnsignedInt248Type;
      } else {
        dt.type = THREE.FloatType;
      }
      dt.name = name;
      return dt;
    };

    inputBuffer.depthTexture = mkDepthTexture('EffectComposer.InputDepth');
    outputBuffer.depthTexture = mkDepthTexture('EffectComposer.OutputDepth');
    inputBuffer.dispose();
    outputBuffer.dispose();
    (this as any).depthRenderTarget = new THREE.WebGLRenderTarget(width, height, {
      depthBuffer: true,
      stencilBuffer: inputBuffer.stencilBuffer,
      depthTexture: mkDepthTexture('EffectComposer.StableDepth'),
    });
  }
}
