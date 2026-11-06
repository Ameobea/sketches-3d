import { EffectComposer } from 'postprocessing';
import type * as THREE from 'three';

/** Exposes postprocessing's private stable depth copy (blitted after every RenderPass; safe to sample). */
export class StableDepthEffectComposer extends EffectComposer {
  get stableDepthTarget(): THREE.WebGLRenderTarget | null {
    return (this as any).depthRenderTarget ?? null;
  }
}
