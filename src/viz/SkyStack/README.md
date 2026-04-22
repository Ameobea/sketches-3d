# SkyStack

A single-pass procedural sky / horizon / ground renderer. One full-screen
fragment shader produces both the tone-mapped sky color and the
emissive-bypass content (stars, building windows, ground paint) in one MRT
draw, slotted into the engine's postprocessing pipeline between the depth
prepass and the main scene render.

## Pipeline placement

```
DepthPass → SkyStackPass → MainRenderPass → ... → EmissiveBypassPass → ... → FinalPass
```

`SkyStackPass` (see `SkyStackPass.ts`) owns:

- `skyMRT` — internal RT with two color attachments, no depth.
- `emissiveRT` — single-attachment RT with depth, **shared with**
  `EmissiveBypassPass` (which composites portal/bypass meshes on top without
  clearing).

Per frame: blit stable depth into `emissiveRT.depth`; clear color on both
RTs; render the unified shader into `skyMRT`; blit attachment 0 →
`inputBuffer` color (gets tone-mapped in `FinalPass`), blit attachment 1 →
`emissiveRT` color (bypasses tone mapping, drives bloom).

The shader's `discardIfOccluded()` reads stableDepth via `uSceneDepth` and
discards any fragment behind scene geometry — so SkyStack only writes where
the sky is actually visible.

## Compositor model: front-to-back accumulation

The shader iterates layers **front to back**, accumulating into:

```glsl
vec3  accumSkyColor;        // → MRT[0] (tone-mapped path)
vec3  accumEmissive;        // → MRT[1] (bypass path)
float accumAlpha;           // running alpha for skyColor
float accumEmissiveAlpha;   // → MRT[1].a (emissive composite weight)
```

Each layer body calls `accumulate(color, emissive, alpha, emissiveAlpha)`
exactly once per contributing fragment. The helper applies the standard
front-to-back porter-duff weighting:

```glsl
weight = 1.0 - accumAlpha;
accumSkyColor      += weight * color;
accumEmissive      += weight * emissive;
accumAlpha         += weight * alpha;
accumEmissiveAlpha += weight * emissiveAlpha;
```

`color` and the alpha-blend channel are **pre-multiplied at the call site**
(e.g. clouds pass `haze.rgb * haze.a`).

Every layer body is wrapped in `if (accumAlpha < SKY_SATURATION_ALPHA)`
(0.999). When any layer pushes accumAlpha past saturation — an opaque
silhouette, a fully-dense cloud, the gradient at the back — every subsequent
layer is skipped. Cross-layer occlusion is mediated entirely through
`accumAlpha`. **No layer references another layer's state.**

This is mathematically equivalent to back-to-front "over" compositing, but
the form unlocks generic occlusion: any layer that emits `alpha=1`
auto-blocks everything behind it without that layer (or the compositor)
needing to know what's back there.

### Why front-to-back

The previous design (see git history before this rewrite) had explicit gate
threading: stars and `cloudsBack` carried `!occludedByBuildings` gates that
referenced a hoisted `BuildingHit` probe. Adding any new occluder would have
required threading a similar flag through every layer that sat behind it.
Front-to-back makes occlusion implicit, and gives predictive cloud
saturation (when `cloudsFront` density hits 1, the entire stack behind it
auto-skips) for free.

## Layer anatomy

Internally, a layer is just:

```ts
interface SkyLayer {
  name: string;
  body: string;          // GLSL that calls accumulate(...) once
  gate?: string;         // optional cheap predicate, e.g. "aboveHorizon"
}
```

Variables in scope inside every body and gate:

| Variable        | Meaning                                              |
|-----------------|------------------------------------------------------|
| `dir`           | normalized world-space view direction                |
| `elev`          | elevation in [-1, 1], 0 = horizon, with horizonOffset|
| `azimuth`       | azimuth in radians                                   |
| `cosElev`       | derived; useful for pole damping                     |
| `horizonBlend`  | smoothstep around the horizon                        |
| `aboveHorizon`  | bool, `elev >= -uHorizonBlend`                       |
| `baseGradient`  | bandless gradient color, computed once per fragment  |

Plus the file-scope accumulators (`accumSkyColor`, `accumEmissive`,
`accumAlpha`, `accumEmissiveAlpha`) and the `accumulate()` helper from the
prelude.

A layer's `gate` is **purely a layer-local perf optimization** to skip the
function call when we know the layer would no-op anyway. It must reference
only compositor-provided variables — never the state of any other layer. If
the gate were referencing other layers it would be a coupling point we're
deliberately avoiding.

## Current layers

Defined in `skyUnifiedShader.ts`, configured via `SkyStackParams`. Front to
back as the compositor sees them:

1. **cloudsFront** — alpha-blend haze in front of buildings (gate: `aboveHorizon`).
2. **buildings** — silhouette + windows. Probe runs inline; on hit, emits
   opaque silhouette (alpha=1) plus window emissive (gate: `aboveHorizon`).
3. **cloudsBack** — alpha-blend haze behind buildings (gate: `aboveHorizon`).
4. **stars** — pure emissive, no skyColor contribution (gate: `aboveHorizon`).
5. **ground** — below-horizon SDF/paint shader, pure emissive
   (gate: `dir.y < 0.01`).
6. **gradient** (always last) — back-most fallback. Outputs alpha=1 to fill
   any remaining coverage. Bands are part of this layer.

Each layer's GLSL helpers + uniforms are only emitted into the assembled
shader source when its config is provided — there's no runtime branching on
layer presence.

## Baked counts

Loop bounds and uniform-array sizes that used to be capped by `MAX_*`
constants are now **baked into the shader at build time** as `#define`s
(`STOP_COUNT`, `BAND_COUNT`, `MAX_HAZE_OCTAVES`). The driver gets literal
loop bounds and can unroll. Counts are immutable per `SkyStack` instance —
`setStops`/`setBands` accept any colors/positions you like but reject
length-mismatched arrays. Reconfiguring the count means recreating the
SkyStack.

## Future direction: generic user-supplied layers

The `SkyLayer { name, body, gate? }` shape and front-to-back accumulator are
deliberately the seed of a generic compose pipeline. The current 5 hardcoded
layers will eventually be replaced (or augmented) by user-supplied layers,
each declaring:

- A z-index for ordering (front-to-back iteration order).
- A GLSL body that calls `accumulate(...)`.
- An optional cheap gate predicate.
- Its own uniforms (passed through to the assembled shader).
- Possibly a blend-mode declaration that auto-derives the `accumulate()` args
  from a higher-level shape (e.g. `kind: 'alphaBlend'` would wrap a `vec4
  source` value as `accumulate(source.rgb * source.a, vec3(0), source.a, 0)`
  without the layer body needing to think about pre-multiplication).

Because cross-layer coupling has already been eliminated, dropping in a new
layer (aurora, lightning flash, distant mountain silhouette, custom
scene-specific overlay) is purely additive — any opaque layer auto-blocks
content behind it via the saturation early-out, and any alpha-blend layer
auto-attenuates emissive behind it via the `(1 - accumAlpha)` weighting.

The current `SkyStackParams`-with-named-slots API will likely become a thin
convenience wrapper that translates the well-known layer kinds into generic
`SkyLayer` entries, with an escape hatch for user-defined layers.

## Performance notes

The big perf wins from the recent optimization pass:

- Above-horizon `paintGround` early-out (the SDF/paint cost was the largest
  per-fragment expense; ~40-50% of pixels are above horizon).
- `skyMRT` no depth attachment (saves a per-frame depth blit).
- Baked loop counts (driver unrolling).
- Front-to-back saturation early-out (predictive cloud / silhouette skip).

Future levers (see `gradient-sky-followups.md` for the longer list):

- Gradient → 1D LUT texture (kills the per-fragment Oklab cube-roots).
- Direct render into `inputBuffer` + `emissiveRT` (skip the two
  intermediate-MRT blits — a real bandwidth saving).
- Quality tier gating (octave reduction, layer skipping at low quality).
- Per-cloud unrolled `skyFbm` (currently shared, with runtime octave count).

## File map

| File | Role |
|------|------|
| `SkyStack.ts` | Public class. Owns uniforms + the `SkyStackPass`. |
| `SkyStackPass.ts` | postprocessing `Pass`. Allocates RTs, runs the draw + blits. |
| `skyUnifiedShader.ts` | Assembles the fragment shader from the configs. Defines the `SkyLayer` shape and the layer bodies. |
| `uniforms.ts` | Shared-uniform definitions, count-aware constructor. |
| `shaders/skyUnified.prelude.frag` | Prelude: MRT outputs, helpers, accumulator + `accumulate()`. |
| `shaders/gradient.glsl` | Gradient-stop interpolation (Oklab) + bands. |
| `shaders/hazeField.glsl` | Cloud band sampler (`sampleHaze` + shared `skyFbm`). |
| `shaders/buildingGeom.glsl` | `probeBuilding()` discrete-tower math. |
| `shaders/ground.glsl` | `sampleGround()` ray-plane intersection + paint dispatch. |
| `shaders/skyStack.vert` | Trivial fullscreen quad vertex shader. |
| `layers/*.ts` | Per-layer config types (interfaces only — no runtime). |
