import type { DefineContribution, Layer, SharedModule } from '../types';
import type * as THREE from 'three';

import { resolveId } from './_util';

export interface CustomLayerConfig {
  id: string;
  zIndex: number;
  uniforms?: Record<string, THREE.IUniform>;
  modules?: SharedModule[];
  defines?: DefineContribution[];
  /**
   * Per-instance GLSL (uniform decls, helper fns). `$ID` tokens are substituted
   * with the layer's id, matching the built-in factories' convention.
   */
  instanceGlsl?: string;
  /**
   * Body GLSL — calls `accumulate(color, emissive, alpha, emissiveAlpha)` once
   * per contributing fragment. `$ID` tokens are substituted with `id`.
   */
  body: string;
  gate?: string;
  /** @see Layer.oversample */
  oversample?: boolean | 2 | 3 | 4;
}

/**
 * Generic layer escape hatch. Authors their own GLSL + uniforms and plugs
 * into the compose pipeline directly. Identical in capability to any of the
 * built-in factories — they're all just `Layer` producers.
 */
export const customLayer = (c: CustomLayerConfig): Layer => ({
  id: c.id,
  zIndex: c.zIndex,
  uniforms: c.uniforms ?? {},
  modules: c.modules,
  defines: c.defines,
  instanceGlsl: c.instanceGlsl ? resolveId(c.instanceGlsl, c.id) : undefined,
  body: resolveId(c.body, c.id),
  gate: c.gate,
  oversample: c.oversample,
});
