import { Pass } from 'postprocessing';
import * as THREE from 'three';

const BLACK = new THREE.Color(0, 0, 0);

/**
 * Single MRT-owning pass that runs the unified SkyStack fragment shader and
 * splits its output into:
 *
 *   - attachment 0 (color)    → blitted into the composer's inputBuffer.
 *                               Tone-mapped by AgX in FinalPass.
 *   - attachment 1 (emissive) → blitted into `emissiveRT` (a standalone single-
 *                               attachment RT owned by this pass). Bypasses
 *                               tone mapping, feeds bloom, and is shared with
 *                               EmissiveBypassPass (which composites bypass
 *                               meshes on top without clearing).
 *
 * Per-frame flow:
 *   1. Blit stableDepthTarget depth → `emissiveRT` depth (skyMRT has no depth
 *      attachment — its shader writes are ungated and the occlusion test
 *      reads stableDepth directly via `uSceneDepth`).
 *   2. Clear `skyMRT` and `emissiveRT` color attachments.
 *   3. Render the fullscreen triangle with the unified material.
 *   4. Blit `skyMRT.textures[0]` → `inputBuffer` color.
 *   5. Blit `skyMRT.textures[1]` → `emissiveRT` color.
 *
 * Placement: immediately between DepthPass and MainRenderPass. As the first
 * non-RenderPass in the composer loop, inserting this pass triggers the
 * StableDepth blit right before it runs — so `stableDepthTarget` holds the
 * freshly-rendered scene depth from DepthPass.
 *
 * `needsSwap = false` — writes to its own MRT + blits into inputBuffer and
 * emissiveRT; the composer's ping-pong is untouched.
 */
export class SkyStackPass extends Pass {
  public readonly skyMRT: THREE.WebGLRenderTarget;
  public readonly emissiveRT: THREE.WebGLRenderTarget;
  private readonly material: THREE.ShaderMaterial;
  private readonly fsScene: THREE.Scene;
  private readonly fsCamera: THREE.OrthographicCamera;
  private readonly fsMesh: THREE.Mesh;
  private stableDepthTarget: THREE.WebGLRenderTarget | null = null;

  constructor(material: THREE.ShaderMaterial, width: number, height: number) {
    super('SkyStackPass');
    this.needsSwap = false;
    this.material = material;

    // No depth attachment on skyMRT: the shader runs with depthTest/depthWrite
    // off, and `discardIfOccluded` samples the *external* stableDepth texture
    // (uSceneDepth) — never this RT's depth. Saves one full-res depth blit and
    // a DepthTexture allocation per frame.
    this.skyMRT = new THREE.WebGLRenderTarget(width, height, {
      count: 2,
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: false,
    });

    this.emissiveRT = new THREE.WebGLRenderTarget(width, height, {
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: true,
      depthTexture: new THREE.DepthTexture(width, height, THREE.UnsignedIntType),
    });

    this.fsScene = new THREE.Scene();
    this.fsCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    this.fsMesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), this.material);
    this.fsMesh.frustumCulled = false;
    this.fsScene.add(this.fsMesh);
  }

  public setStableDepthTarget(target: THREE.WebGLRenderTarget): void {
    this.stableDepthTarget = target;
  }

  override setSize(width: number, height: number): void {
    this.skyMRT.setSize(width, height);
    this.emissiveRT.setSize(width, height);
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    _outputBuffer: THREE.WebGLRenderTarget
  ): void {
    const gl = renderer.getContext() as WebGL2RenderingContext;
    const props = (renderer as any).properties;

    // Force FBO allocation for emissiveRT before the raw blit. skyMRT has no
    // depth attachment so it doesn't need a depth blit — its color writes are
    // ungated and the shader's occlusion test reads stableDepth directly via
    // uSceneDepth.
    renderer.setRenderTarget(this.emissiveRT);

    // Blit stable depth into emissiveRT.depth — that's what EmissiveBypassPass
    // depth-tests its bypass meshes against later in the composer.
    if (this.stableDepthTarget) {
      const srcProps = props.get(this.stableDepthTarget);
      const emProps = props.get(this.emissiveRT);
      if (srcProps?.__webglFramebuffer && emProps?.__webglFramebuffer) {
        const srcFBO = srcProps.__webglFramebuffer as WebGLFramebuffer;
        const emFBO = emProps.__webglFramebuffer as WebGLFramebuffer;
        const { width, height } = this.emissiveRT;

        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, srcFBO);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, emFBO);
        gl.blitFramebuffer(0, 0, width, height, 0, 0, width, height, gl.DEPTH_BUFFER_BIT, gl.NEAREST);

        gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
      }
      renderer.setRenderTarget(null);
    }

    // Clear MRT color (both attachments). No depth on this RT, so a color-only
    // clear is all that's needed.
    renderer.setRenderTarget(this.skyMRT);
    renderer.setClearColor(BLACK, 0);
    renderer.clearColor();

    // Also clear the emissiveRT color. The attachment[1] blit below overwrites
    // it, but clearing explicitly also handles the case where the shader
    // early-discards every fragment (nothing to blit over stale pixels).
    renderer.setRenderTarget(this.emissiveRT);
    renderer.clearColor();

    // Draw the unified shader into the MRT.
    renderer.setRenderTarget(this.skyMRT);
    renderer.render(this.fsScene, this.fsCamera);

    // Blit MRT attachments into their consumer RTs.
    const inputProps = props.get(inputBuffer);
    const skyProps = props.get(this.skyMRT);
    const emProps = props.get(this.emissiveRT);
    if (skyProps?.__webglFramebuffer) {
      const { width, height } = this.skyMRT;
      const srcFBO = skyProps.__webglFramebuffer as WebGLFramebuffer;
      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, srcFBO);

      if (inputProps?.__webglFramebuffer) {
        const dstFBO = inputProps.__webglFramebuffer as WebGLFramebuffer;
        gl.readBuffer(gl.COLOR_ATTACHMENT0);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, dstFBO);
        // inputBuffer is single-attachment — its draw buffer is already
        // COLOR_ATTACHMENT0, no need to reset.
        gl.blitFramebuffer(0, 0, width, height, 0, 0, width, height, gl.COLOR_BUFFER_BIT, gl.NEAREST);
      }

      if (emProps?.__webglFramebuffer) {
        const dstFBO = emProps.__webglFramebuffer as WebGLFramebuffer;
        gl.readBuffer(gl.COLOR_ATTACHMENT1);
        gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, dstFBO);
        gl.blitFramebuffer(0, 0, width, height, 0, 0, width, height, gl.COLOR_BUFFER_BIT, gl.NEAREST);
      }

      gl.bindFramebuffer(gl.READ_FRAMEBUFFER, null);
      gl.bindFramebuffer(gl.DRAW_FRAMEBUFFER, null);
    }

    renderer.setRenderTarget(null);
  }

  override dispose(): void {
    this.skyMRT.dispose();
    this.emissiveRT.dispose();
    this.fsMesh.geometry.dispose();
    this.material.dispose();
    super.dispose();
  }
}
