import { Pass } from 'postprocessing';
import * as THREE from 'three';

const EMISSIVE_CLEAR = new Float32Array([0, 0, 0, 0]);

/**
 * MRT-owning pass that runs the unified SkyStack fragment shader and writes
 * directly into the consumer render targets:
 *
 *   - attachment 0 (color)
 *   - attachment 1 (emissiveBypass)
 *
 * Implementation: a single three.js-managed MRT (`skyMRT`) whose two color
 * attachments are rebound each frame to the consumer RTs' underlying GL
 * textures.
 *
 * Per-frame flow:
 *   1. Rebind skyMRT's COLOR_ATTACHMENT0/1 to inputBuffer.texture and
 *      emissiveRT.texture's GL textures (only when those textures change —
 *      first run, resize, or RT reallocation).
 *   2. Clear only attachment 1 — attachment 0 holds the scene color that
 *      MainRenderPass just wrote and `discardIfOccluded()` will preserve it
 *      at geometry pixels.
 *   3. Render the fullscreen triangle; fragment outputs land directly in
 *      inputBuffer and emissiveRT.
 *
 * Placement: AFTER MainRenderPass, so the StableDepth blit (triggered by
 * the first non-RenderPass) captures the full scene depth including
 * materials with `skipDepthPrepass` like bounded POM — otherwise the sky
 * would draw over those pixels.
 *
 * `emissiveRT` depth: this pass constructs `emissiveRT` without a depth
 * attachment, and a wiring step in `defaultPostprocessing` attaches
 * `stableDepthTarget.depthTexture` directly as `emissiveRT`'s depth via
 * `setEmissiveDepthTexture()`.
 */
export class SkyStackPass extends Pass {
  public readonly skyMRT: THREE.WebGLRenderTarget;
  public readonly emissiveRT: THREE.WebGLRenderTarget;
  private readonly material: THREE.ShaderMaterial;
  private readonly fsScene: THREE.Scene;
  private readonly fsCamera: THREE.OrthographicCamera;
  private readonly fsMesh: THREE.Mesh;
  private boundAttachment0: WebGLTexture | null = null;
  private boundAttachment1: WebGLTexture | null = null;

  constructor(material: THREE.ShaderMaterial, width: number, height: number) {
    super('SkyStackPass');
    this.needsSwap = false;
    this.material = material;

    // Backing MRT whose color attachments get hijacked each frame. We still
    // let three.js construct it — that gives us a valid FBO + drawBuffers
    // state + viewport handling via renderer.setRenderTarget/render. The
    // internal textures it allocates are overwritten by our external
    // attachments and never read, but they're small enough to live with.
    this.skyMRT = new THREE.WebGLRenderTarget(width, height, {
      count: 2,
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: false,
    });

    // No depth attachment on construction. `setEmissiveDepthTexture()` wires
    // stableDepth's depth texture in before first render
    this.emissiveRT = new THREE.WebGLRenderTarget(width, height, {
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: false,
    });

    this.fsScene = new THREE.Scene();
    this.fsCamera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1);
    this.fsMesh = new THREE.Mesh(new THREE.PlaneGeometry(2, 2), this.material);
    this.fsMesh.frustumCulled = false;
    this.fsScene.add(this.fsMesh);
  }

  /**
   * Attach an externally-owned depth texture as `emissiveRT`'s depth.
   * MUST be called before the first render: three.js sets up the FBO (and
   * attaches whatever depth is present on the RT at that moment) lazily on
   * the first `setRenderTarget(emissiveRT)`.
   */
  public setEmissiveDepthTexture(depthTexture: THREE.DepthTexture): void {
    this.emissiveRT.depthTexture = depthTexture;
    this.emissiveRT.depthBuffer = true;
  }

  override setSize(width: number, height: number): void {
    this.skyMRT.setSize(width, height);
    this.emissiveRT.setSize(width, height);
    // setSize disposes and forces re-allocation of the underlying GL textures
    // on next setRenderTarget, so the attachment identities we cached are
    // stale. Force re-bind on next render().
    this.boundAttachment0 = null;
    this.boundAttachment1 = null;
  }

  override render(
    renderer: THREE.WebGLRenderer,
    inputBuffer: THREE.WebGLRenderTarget,
    _outputBuffer: THREE.WebGLRenderTarget
  ): void {
    if (!this.emissiveRT.depthTexture) {
      throw new Error(
        "SkyStackPass: setEmissiveDepthTexture() must be called before the first render so emissiveRT shares the composer's stable depth — otherwise the FBO is created without depth and bypass meshes drawn on top of the sky have nothing to depth-test against."
      );
    }

    const gl = renderer.getContext() as WebGL2RenderingContext;
    const props = (renderer as any).properties;

    // Ensure both consumer RTs have allocated GL textures — we borrow them
    // as attachments below.
    renderer.setRenderTarget(inputBuffer);
    renderer.setRenderTarget(this.emissiveRT);

    // Bind skyMRT once so three.js allocates its FBO if it hasn't already.
    renderer.setRenderTarget(this.skyMRT);

    // Hijack skyMRT's color attachments to point directly at the consumer
    // RTs' GL textures. This is what lets renderer.render() below write
    // straight into inputBuffer + emissiveRT without a blit.
    const skyProps = props.get(this.skyMRT);
    const inputTexProps = props.get(inputBuffer.texture);
    const emTexProps = props.get(this.emissiveRT.texture);
    const skyFBO = skyProps?.__webglFramebuffer as WebGLFramebuffer | undefined;
    const inputGLTex = inputTexProps?.__webglTexture as WebGLTexture | undefined;
    const emGLTex = emTexProps?.__webglTexture as WebGLTexture | undefined;

    if (
      skyFBO &&
      inputGLTex &&
      emGLTex &&
      (this.boundAttachment0 !== inputGLTex || this.boundAttachment1 !== emGLTex)
    ) {
      gl.bindFramebuffer(gl.FRAMEBUFFER, skyFBO);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, inputGLTex, 0);
      gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT1, gl.TEXTURE_2D, emGLTex, 0);
      gl.bindFramebuffer(gl.FRAMEBUFFER, null);
      this.boundAttachment0 = inputGLTex;
      this.boundAttachment1 = emGLTex;
    }

    // Clear only attachment 1; attachment 0 holds MainRenderPass's scene
    // color which discarded sky fragments must leave intact.
    renderer.setRenderTarget(this.skyMRT);
    gl.clearBufferfv(gl.COLOR, 1, EMISSIVE_CLEAR);

    renderer.render(this.fsScene, this.fsCamera);

    // Skip setRenderTarget(null) — the next pass rebinds its own target.
  }

  override dispose(): void {
    this.skyMRT.dispose();
    this.emissiveRT.dispose();
    this.fsMesh.geometry.dispose();
    this.material.dispose();
    super.dispose();
  }
}
