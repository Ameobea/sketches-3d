import * as THREE from 'three';

import type { Layer } from '../types';
import { resolveId } from './_util';
import groundGlsl from './ground.glsl?raw';

export interface GroundLayerConfig {
  id: string;
  zIndex: number;
  /**
   * UV-space scale knob for the virtual ground plane. Larger values make
   * features appear smaller. Default 100.
   */
  height?: number;
  /** Below-horizon elevation (in |dir.y| units) where alpha fade begins. Default 0. */
  horizonFadeStart?: number;
  /** Below-horizon elevation where alpha fade completes. Default 0.08. */
  horizonFadeEnd?: number;
  /**
   * GLSL source defining the paint function:
   *   `vec4 paintGround_$ID(vec2 uv, vec2 uvDeriv, vec3 dir, float invDist)`
   * The `$ID` token is substituted with this layer's id before compilation,
   * so multiple ground layers don't collide. `uTime` and helpers from
   * `noise.frag` are in scope.
   */
  paintShader: string;
  /**
   * Fake atmospheric tint — mixes paint toward `color` as -dir.y approaches
   * 0. Disabled at strength 0 (the default).
   */
  atmosphericTint?: {
    color: THREE.ColorRepresentation;
    /** |dir.y| range over which the tint fades in. Default 0.2. */
    range?: number;
    /** Max mix factor at the horizon. Default 0. */
    strength?: number;
  };
  /** Additional uniforms exposed to the paint shader. */
  uniforms?: Record<string, THREE.IUniform>;
  /** @see Layer.oversample */
  oversample?: boolean;
}

export const groundLayer = (c: GroundLayerConfig): Layer => {
  const id = c.id;
  const uniforms: Record<string, THREE.IUniform> = {
    [`uGroundHeight_${id}`]: { value: c.height ?? 100 },
    [`uGroundHorizonFadeStart_${id}`]: { value: c.horizonFadeStart ?? 0.0 },
    [`uGroundHorizonFadeEnd_${id}`]: { value: c.horizonFadeEnd ?? 0.08 },
    [`uGroundAtmoTintColor_${id}`]: { value: new THREE.Color(c.atmosphericTint?.color ?? 0x000000) },
    [`uGroundAtmoTintRange_${id}`]: { value: c.atmosphericTint?.range ?? 0.2 },
    [`uGroundAtmoTintStrength_${id}`]: { value: c.atmosphericTint?.strength ?? 0 },
    ...(c.uniforms ?? {}),
  };

  // User paint shader first (defines `paintGround_$ID`), then the sampler.
  const instanceGlsl = [
    `// === ${id} paintShader ===\n${resolveId(c.paintShader, id)}`,
    resolveId(groundGlsl, id),
  ].join('\n');

  return {
    id,
    zIndex: c.zIndex,
    uniforms,
    instanceGlsl,
    body: resolveId(
      `// sampleGround's internal dir.y > 0.01 check is a safety net; the outer
      // gate (dir.y < 0.01) keeps us out of it from the unsafe side. Ground is
      // emissive-only — alpha=0 so it doesn't block the background below.
      vec4 g;
      sampleGround_$ID(dir, g);
      accumulate(vec3(0.0), g.rgb * g.a, 0.0, g.a);`,
      id
    ),
    gate: 'dir.y < 0.01',
    oversample: c.oversample,
  };
};
