import { Pass } from 'postprocessing';
import * as THREE from 'three';

const BLACK = new THREE.Color(0, 0, 0);

/**
 * MRT-owning pass that runs the unified SkyStack fragment shader and writes
 * directly into the consumer render targets:
 *
 *   - attachment 0 (color)    → `inputBuffer.texture` (tone-mapped by AgX in FinalPass).
 *   - attachment 1 (emissive) → `emissiveRT.texture`  (bypass-tone-map + bloom,
 *                               shared with EmissiveBypassPass which composites on top
 *                               without clearing).
 *
 * Implementation: a single three.js-managed MRT (`skyMRT`) whose two color
 * attachments are rebound each frame to the consumer RTs' underlying GL
 * textures. This eliminates the two per-frame full-resolution color blits
 * that the previous "render into skyMRT then blit out" design required.
 * On TBDR hardware (Apple Silicon) those blits forced tile-memory resolves
 * that hitched any pass sharing `inputBuffer` downstream (e.g. n8ao).
 *
 * Per-frame flow:
 *   1. Rebind skyMRT's COLOR_ATTACHMENT0/1 to inputBuffer.texture and
 *      emissiveRT.texture's GL textures (only when those textures change —
 *      first run, resize, or RT reallocation).
 *   2. Clear skyMRT (clears through the hijacked attachments → clears
 *      inputBuffer.color and emissiveRT.color).
 *   3. Render the fullscreen triangle with the unified material — fragment
 *      outputs go directly into inputBuffer and emissiveRT.
 *
 * Placement: between DepthPass and MainRenderPass. As the first non-RenderPass
 * in the composer loop, inserting this pass triggers the StableDepth blit
 * right before it runs — so `stableDepthTarget` holds the freshly-rendered
 * scene depth from DepthPass (consumed by the sky shader via `uSceneDepth`).
 *
 * `emissiveRT` depth: this pass constructs `emissiveRT` without a depth
 * attachment, and a wiring step in `defaultPostprocessing` attaches
 * `stableDepthTarget.depthTexture` directly as `emissiveRT`'s depth via
 * `setEmissiveDepthTexture()`. Result: no per-frame depth blit. The
 * EmissiveBloomPass filter step (when fog is active) gets "mesh depth at
 * mesh pixels, scene depth elsewhere" semantics because bypass meshes
 * render with depthWrite=true into the shared texture; the only cost is
 * that FinalPass's fog reads the same shared texture and therefore sees
 * mesh depth at bypass-mesh pixels — invisible for opaque bypass meshes
 * (the common case), since the bypass emissive covers the main-scene
 * color at those pixels in the final composite.
 *
 * `needsSwap = false` — does not rotate the composer's ping-pong.
 */
export class SkyStackPass extends Pass {
  public readonly skyMRT: THREE.WebGLRenderTarget;
  public readonly emissiveRT: THREE.WebGLRenderTarget;
  private readonly material: THREE.ShaderMaterial;
  private readonly fsScene: THREE.Scene;
  private readonly fsCamera: THREE.OrthographicCamera;
  private readonly fsMesh: THREE.Mesh;
  // Tracks which GL textures are currently attached to skyMRT's color slots so
  // we only re-run framebufferTexture2D when the consumer textures actually
  // change (resize / first frame / RT reallocation).
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
    // stableDepth's depth texture in before first render; emissiveRT then
    // shares that depth, eliminating the per-frame blit.
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

    // Clear both attachments (→ clears inputBuffer.color and emissiveRT.color
    // since those are what's attached). Sky-occluded fragments will discard
    // and leave the cleared value; MainRenderPass writes scene color on top.
    renderer.setRenderTarget(this.skyMRT);
    renderer.setClearColor(BLACK, 0);
    renderer.clearColor();

    // Render fragment outputs go straight into the consumer RT textures.
    renderer.render(this.fsScene, this.fsCamera);

    // Intentionally NOT calling setRenderTarget(null) here — the next pass
    // (MainRenderPass) immediately rebinds inputBuffer, so a default-FBO
    // bind in between is wasted work. On TBDR (Apple Silicon) it also
    // forces an unnecessary tile-memory resolve to the default framebuffer.
  }

  override dispose(): void {
    this.skyMRT.dispose();
    this.emissiveRT.dispose();
    this.fsMesh.geometry.dispose();
    this.material.dispose();
    super.dispose();
  }
}
