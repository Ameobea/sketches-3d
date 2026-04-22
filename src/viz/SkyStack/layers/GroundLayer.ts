import type * as THREE from 'three';

export interface GroundLayerConfig {
  /**
   * UV-space scale knob for the virtual ground plane. The plane is
   * mathematical, not literal geometry — larger `height` makes features
   * appear smaller (coarser UV units), smaller makes them larger. Start
   * around 50–200 and tune by eye. Default 100.
   */
  height?: number;
  /**
   * Below-horizon elevation (in |dir.y| units) where alpha fade begins.
   * 0 means the fade starts exactly at the horizon line. Default 0.
   */
  horizonFadeStart?: number;
  /**
   * Below-horizon elevation where alpha fade completes. Default 0.08 —
   * roughly the bottom 8% of the view direction range below horizon.
   */
  horizonFadeEnd?: number;
  /**
   * GLSL source that defines the paint function:
   *
   *   `vec4 paintGround(vec2 uv, vec2 uvDeriv, vec3 dir, float invDist)`
   *
   * Returns (rgb, alpha). Alpha is multiplied by the horizon fade. `uTime`
   * and helpers from `noise.frag` (hash, noise, fbm) are in scope.
   */
  paintShader: string;
  /**
   * Fake atmospheric tint — mixes the paint color toward `color` as -dir.y
   * approaches 0, approximating distance reddening/darkening without a real
   * optical-depth calc. At strength 0 (the default) it's a no-op.
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
}
