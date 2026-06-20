import * as THREE from 'three';

import { HijackedMRTPass } from 'src/viz/passes/hijackedMRTPass';

/**
 * Runs the unified SkyStack fragment shader and writes directly into the consumer
 * render targets — attachment 0 (scene color = `inputBuffer`) and attachment 1
 * (`emissiveRT`) — via the backing-MRT attachment hijack in `HijackedMRTPass`.
 *
 * Per-frame flow:
 *   1. `bindAttachments` re-points the backing MRT at inputBuffer + emissiveRT.
 *   2. Render the fullscreen triangle; its outputs land in inputBuffer + emissiveRT.
 *      attachment 1 was zeroed by `EmissiveClearPass` ahead of this; `discardIfOccluded()`
 *      leaves both attachments at their prior values at geometry pixels.
 *
 * The backing MRT stays depthless (the sky doesn't depth-test; occlusion is via
 * `discardIfOccluded()`). `setEmissiveDepthTexture()` instead wires stableDepth
 * onto the CONSUMER `emissiveRT` for the later passes that render into it.
 *
 * Placement: AFTER MainRenderPass, so the StableDepth blit (triggered by the first
 * non-RenderPass) captures full scene depth including `skipDepthPrepass` materials
 * like bounded POM — otherwise the sky would draw over those pixels.
 */
export class SkyStackPass extends HijackedMRTPass {
  public readonly emissiveRT: THREE.WebGLRenderTarget;
  private readonly material: THREE.ShaderMaterial;
  private readonly fsScene: THREE.Scene;
  private readonly fsCamera: THREE.OrthographicCamera;
  private readonly fsMesh: THREE.Mesh;

  constructor(material: THREE.ShaderMaterial, width: number, height: number) {
    super('SkyStackPass', width, height);
    this.material = material;

    // No depth attachment on construction. `setEmissiveDepthTexture()` wires
    // stableDepth's depth texture in before first render.
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
   * Attach an externally-owned depth texture as `emissiveRT`'s depth. MUST be
   * called before the first render: three sets up the FBO (and attaches whatever
   * depth is present) lazily on the first `setRenderTarget(emissiveRT)`.
   */
  public setEmissiveDepthTexture(depthTexture: THREE.DepthTexture): void {
    this.emissiveRT.depthTexture = depthTexture;
    this.emissiveRT.depthBuffer = true;
  }

  override setSize(width: number, height: number): void {
    super.setSize(width, height);
    this.emissiveRT.setSize(width, height);
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

    if (!this.bindAttachments(renderer, inputBuffer, this.emissiveRT)) {
      return;
    }

    // No clear here: `EmissiveClearPass` zeroed emissiveRT (attachment 1) ahead of
    // this pass, and attachment 0 holds MainRenderPass's scene color, which
    // discarded sky fragments must leave intact.
    renderer.render(this.fsScene, this.fsCamera);

    // Skip setRenderTarget(null) — the next pass rebinds its own target.
  }

  override dispose(): void {
    this.emissiveRT.dispose();
    this.fsMesh.geometry.dispose();
    this.material.dispose();
    super.dispose();
  }
}
