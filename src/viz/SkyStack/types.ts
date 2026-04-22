import type * as THREE from 'three';

/**
 * Core types for the SkyStack compose pipeline.
 *
 * The compositor runs a list of `Layer`s front-to-back (highest `zIndex`
 * first), wrapping each body in a saturation early-out. After the last
 * layer, an optional `BackgroundLayer` runs as the guaranteed alpha=1
 * backmost fill — if absent, the sky defaults to black.
 *
 * Every layer writes through the `accumulate(color, emissive, alpha,
 * emissiveAlpha)` helper (prelude.frag). `color` and the alpha-blend
 * channel are pre-multiplied at the call site.
 *
 * Compositor-scope GLSL variables available to every body and gate:
 *   dir, elev, azimuth, cosElev, horizonBlend, aboveHorizon
 * Plus the accumulators (`accumSkyColor`, `accumEmissive`, `accumAlpha`,
 * `accumEmissiveAlpha`) from the prelude.
 */

/**
 * A GLSL chunk (functions, uniform decls, helpers) shared across layers.
 * Modules are deduplicated by `key`: if multiple layers contribute a module
 * with the same key, it's emitted exactly once. The bodies must agree — a
 * mismatch is an authoring error and throws at compose time.
 */
export interface SharedModule {
  key: string;
  glsl: string;
}

/**
 * A `#define` contributed to the shader. Multiple layers may contribute
 * the same key; the composer merges them with the declared `merge` op.
 * Every contribution for a given key must use the same op.
 */
export interface DefineContribution {
  key: string;
  value: number;
  merge: 'max' | 'sum';
}

/**
 * A generic sky layer. Factory functions (starsLayer, cloudsLayer, …) build
 * these from typed configs; a bare `Layer` is also the escape hatch for
 * user-defined layers via `customLayer(...)`.
 */
export interface Layer {
  /** Unique across all layers + background. Used to disambiguate uniforms and helper fns. */
  id: string;
  /** CSS-like: higher = closer to camera = emitted earlier (front-to-back). */
  zIndex: number;
  uniforms: Record<string, THREE.IUniform>;
  /** Deduped-by-key GLSL emitted once at file scope. */
  modules?: SharedModule[];
  /** Compile-time constants contributed to the shader; merged across all layers. */
  defines?: DefineContribution[];
  /** Per-instance GLSL (uniform decls, helper fns) emitted at file scope after modules. */
  instanceGlsl?: string;
  /**
   * GLSL inserted into main(), wrapped by the composer in
   *   `if (accumAlpha < SKY_SATURATION_ALPHA) { [if (gate)] { body } }`.
   * Must call `accumulate(...)` once per contributing fragment (zero or one).
   */
  body: string;
  /**
   * Optional cheap predicate (free-form GLSL expression) that gates the body.
   * Purely a per-layer perf optimization — may reference only compositor-scope
   * variables, never another layer's state.
   */
  gate?: string;
}

/**
 * The single optional backmost layer. Contract:
 *   - always runs last, regardless of zIndex (no zIndex field)
 *   - must emit `alpha=1` via `accumulate(...)` to saturate the stack
 *   - no gate (always runs when reached)
 * Omit entirely for a black sky.
 */
export interface BackgroundLayer {
  id: string;
  uniforms: Record<string, THREE.IUniform>;
  modules?: SharedModule[];
  defines?: DefineContribution[];
  instanceGlsl?: string;
  body: string;
}
