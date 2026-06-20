import { Pass } from 'postprocessing';
import * as THREE from 'three';

const BLACK = new THREE.Color(0, 0, 0);

/**
 * Clears the shared emissive-bypass RT (color only) once per frame, before any
 * producer writes it. Centralizes what was an implicit "first producer clears"
 * contract spread across `SkyStackPass` + `EmissiveBypassPass`: with this pass in
 * the pipeline ahead of them, every emissive producer is a pure compositor.
 *
 * Color only — `emissiveRT`'s depth attachment is the composer's shared stable
 * depth and must persist for the producers to depth-test against. Non-owning: the
 * RT is allocated/disposed by whoever created it (SkyStack or EmissiveBypass).
 */
export class EmissiveClearPass extends Pass {
  private readonly emissiveRT: THREE.WebGLRenderTarget;

  constructor(emissiveRT: THREE.WebGLRenderTarget) {
    super('EmissiveClearPass');
    this.needsSwap = false;
    this.emissiveRT = emissiveRT;
  }

  override render(renderer: THREE.WebGLRenderer): void {
    renderer.setRenderTarget(this.emissiveRT);
    renderer.setClearColor(BLACK, 0);
    renderer.clear(true, false, false);
  }
}
