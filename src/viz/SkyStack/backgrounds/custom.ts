import type * as THREE from 'three';

import { resolveId } from '../layers/_util';
import type { BackgroundLayer, DefineContribution, SharedModule } from '../types';

export interface CustomBackgroundConfig {
  id: string;
  uniforms?: Record<string, THREE.IUniform>;
  modules?: SharedModule[];
  defines?: DefineContribution[];
  /** `$ID` tokens are substituted with `id`. */
  instanceGlsl?: string;
  /**
   * Body GLSL. Contract: must call `accumulate(...)` with `alpha=1` so the
   * stack saturates. `$ID` is substituted with `id`.
   */
  body: string;
}

/** Background escape hatch — same shape as `customLayer` but without zIndex/gate. */
export const customBackground = (c: CustomBackgroundConfig): BackgroundLayer => ({
  id: c.id,
  uniforms: c.uniforms ?? {},
  modules: c.modules,
  defines: c.defines,
  instanceGlsl: c.instanceGlsl ? resolveId(c.instanceGlsl, c.id) : undefined,
  body: resolveId(c.body, c.id),
});
