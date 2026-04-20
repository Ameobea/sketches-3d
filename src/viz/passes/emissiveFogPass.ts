import { Pass } from 'postprocessing';
import * as THREE from 'three';

import type { Viz } from '..';
import type { EmissiveBypassPass } from './emissiveBypassPass';
import FRAGMENT_SHADER from './shaders/emissiveFog.frag?raw';
import VERTEX_SHADER from './shaders/emissiveFog.vert?raw';

/**
 * Applies the user-supplied fog function to the emissive bypass RT, producing a fogged
 * emissive texture that is consumed by both the emissive bloom pass and the final-pass
 * composite.
 *
 * Why a dedicated pass (rather than folding fog into FinalPass):
 *   FinalPass composites a pre-blurred bloom texture additively. If the blur source is the
 *   un-fogged emissive, there is no way at composite time to correctly attenuate halo pixels
 *   where the halo overlaps un-fogged foreground geometry — attenuating by the output pixel's
 *   depth produces sheen artifacts, and attenuating by the source pixel's depth is impossible
 *   because the blur has smeared across many source pixels. Fogging the emissive first and
 *   feeding that into the blur makes all downstream compositing correct for free.
 *
 * Efficiency:
 *   - This pass is only constructed when both emissive bypass AND a custom fog shader are
 *     active (gated in defaultPostprocessing.ts).
 *   - The output RT's GPU framebuffer is allocated lazily by Three.js on first
 *     `setRenderTarget`, so if no frame ever renders an emissive bypass mesh in-frustum, the
 *     FBO is never created.
 *   - When the upstream bypass pass reports `hasContent === false` the fog shader is skipped
 *     entirely. On the transition from content → no-content we do a one-time clear so stale
 *     pixels from the previous frame don't leak into downstream consumers.
 *
 * needsSwap = false — writes to its own RT, not the ping-pong pair.
 */
export class EmissiveFogPass extends Pass {
  readonly fogEmissiveRT: THREE.WebGLRenderTarget;
  private readonly mat: THREE.ShaderMaterial;
  private readonly viz: Viz;
  private readonly bypassPass: EmissiveBypassPass;
  private outHasContent = false;

  constructor(viz: Viz, bypassPass: EmissiveBypassPass, fogShader: string, width: number, height: number) {
    super('EmissiveFogPass');
    this.needsSwap = false;
    this.viz = viz;
    this.bypassPass = bypassPass;

    this.fogEmissiveRT = new THREE.WebGLRenderTarget(width, height, {
      type: THREE.HalfFloatType,
      format: THREE.RGBAFormat,
      depthBuffer: false,
    });

    this.mat = new THREE.ShaderMaterial({
      name: 'EmissiveFogMaterial',
      uniforms: {
        emissiveBuffer: { value: bypassPass.emissiveRT.texture },
        emissiveDepthBuffer: { value: bypassPass.emissiveRT.depthTexture },
        // Camera matrices are held by reference so they stay in sync with the live camera
        // without any per-frame copy. Three reads `uniform.value` by reference each draw.
        projectionMatrixInverse: { value: viz.camera.projectionMatrixInverse },
        cameraWorldMatrix: { value: viz.camera.matrixWorld },
        fogCameraPos: { value: new THREE.Vector3() },
        fogPlayerPos: { value: new THREE.Vector3() },
        curTimeSeconds: { value: 0.0 },
      },
      vertexShader: VERTEX_SHADER,
      fragmentShader: fogShader + '\n' + FRAGMENT_SHADER,
      depthWrite: false,
      depthTest: false,
    });
    this.fullscreenMaterial = this.mat;
  }

  override setSize(width: number, height: number): void {
    this.fogEmissiveRT.setSize(width, height);
  }

  override render(
    renderer: THREE.WebGLRenderer,
    _inputBuffer: THREE.WebGLRenderTarget,
    _outputBuffer: THREE.WebGLRenderTarget
  ): void {
    if (!this.bypassPass.hasContent) {
      if (this.outHasContent) {
        renderer.setRenderTarget(this.fogEmissiveRT);
        renderer.setClearColor(new THREE.Color(0, 0, 0), 0);
        renderer.clear(true, false, false);
        renderer.setRenderTarget(null);
        this.outHasContent = false;
      }
      return;
    }

    this.mat.uniforms.fogCameraPos.value.setFromMatrixPosition(this.viz.camera.matrixWorld);
    if (this.viz.fpCtx) {
      this.mat.uniforms.curTimeSeconds.value = this.viz.fpCtx.getPhysicsTime();
      const playerPos = this.viz.fpCtx.playerController.getPosition();
      if (playerPos) {
        this.mat.uniforms.fogPlayerPos.value.set(playerPos.x(), playerPos.y(), playerPos.z());
      }
    }

    renderer.setRenderTarget(this.fogEmissiveRT);
    renderer.render(this.scene, this.camera);
    this.outHasContent = true;
  }

  override dispose(): void {
    this.fogEmissiveRT.dispose();
    this.mat.dispose();
    super.dispose();
  }
}
