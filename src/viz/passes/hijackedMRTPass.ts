import { Pass } from 'postprocessing';
import * as THREE from 'three';

/**
 * Base for passes that render scene color + emissive into two externally-owned
 * render targets in a single draw, with no copy. It owns a 2-attachment "backing"
 * MRT whose color attachments are re-pointed each frame at the consumer RTs'
 * underlying GL textures via raw `framebufferTexture2D`, so one
 * `renderer.render(...)` into the backing MRT lands straight in both consumers.
 *
 * Subclasses call `bindAttachments(renderer, colorRT, emissiveRT)` — which leaves
 * the backing MRT bound as the active target — then issue their own clears + draw.
 * A subclass that depth-tests/-writes calls `attachDepth()` once before the first
 * render; otherwise the backing MRT stays depthless.
 *
 * This deliberately reaches into three.js internals (`__webglFramebuffer` /
 * `__webglTexture`). It holds because a consumer RT's GL texture is stable for a
 * fixed pipeline, and the rebind guard catches resize / RT reallocation.
 */
export abstract class HijackedMRTPass extends Pass {
  protected readonly backingMRT: THREE.WebGLRenderTarget;
  private boundAttachment0: WebGLTexture | null = null;
  private boundAttachment1: WebGLTexture | null = null;

  constructor(name: string, width: number, height: number) {
    super(name);
    this.needsSwap = false;
    this.backingMRT = new THREE.WebGLRenderTarget(width, height, {
      count: 2,
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: false,
    });
  }

  /**
   * Attach a depth texture to the backing MRT (to depth-test/-write against the
   * shared scene depth). MUST be called before the first render — three creates
   * the FBO lazily on the first `setRenderTarget(backingMRT)`.
   */
  protected attachDepth(depthTexture: THREE.DepthTexture): void {
    this.backingMRT.depthTexture = depthTexture;
    this.backingMRT.depthBuffer = true;
  }

  /**
   * Point the backing MRT's COLOR_ATTACHMENT0/1 at `colorRT.texture` and
   * `emissiveRT.texture`'s GL textures and leave the backing MRT bound as the
   * active render target. Rebinds only when those textures change (first run,
   * resize, RT realloc). Returns false if a GL texture isn't allocated yet — the
   * caller must bail without drawing.
   */
  protected bindAttachments(
    renderer: THREE.WebGLRenderer,
    colorRT: THREE.WebGLRenderTarget,
    emissiveRT: THREE.WebGLRenderTarget
  ): boolean {
    const gl = renderer.getContext() as WebGL2RenderingContext;
    const props = (renderer as any).properties;

    // Force allocation of the consumer RTs' GL textures, then bind the backing MRT
    // so three allocates its FBO (+ attaches depth) if it hasn't yet.
    renderer.setRenderTarget(colorRT);
    renderer.setRenderTarget(emissiveRT);
    renderer.setRenderTarget(this.backingMRT);

    const mrtFBO = props.get(this.backingMRT)?.__webglFramebuffer as WebGLFramebuffer | undefined;
    const tex0 = props.get(colorRT.texture)?.__webglTexture as WebGLTexture | undefined;
    const tex1 = props.get(emissiveRT.texture)?.__webglTexture as WebGLTexture | undefined;
    if (!mrtFBO || !tex0 || !tex1) {
      return false;
    }

    if (this.boundAttachment0 !== tex0 || this.boundAttachment1 !== tex1) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, mrtFBO);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, tex0, 0);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT1, gl.TEXTURE_2D, tex1, 0);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      this.boundAttachment0 = tex0;
      this.boundAttachment1 = tex1;
    }

    // Re-bind (the rebind above unbinds to null) so three's state + drawBuffers are
    // set for the caller's render.
    renderer.setRenderTarget(this.backingMRT);
    return true;
  }

  override setSize(width: number, height: number): void {
    this.backingMRT.setSize(width, height);
    this.boundAttachment0 = null;
    this.boundAttachment1 = null;
  }

  override dispose(): void {
    this.backingMRT.dispose();
    super.dispose();
  }
}
