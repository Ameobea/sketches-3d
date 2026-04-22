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

- `skyMRT` — three.js-managed MRT whose two color attachments are rebound
  each frame to the consumer RTs' underlying GL textures (see below).
- `emissiveRT` — single-attachment RT with depth, **shared with**
  `EmissiveBypassPass` (which composites portal/bypass meshes on top without
  clearing, and owns the blit that populates `emissiveRT.depth`).

Per frame: rebind `skyMRT`'s color attachments to `inputBuffer.texture` and
`emissiveRT.texture`'s GL textures (cache-skipped when unchanged); clear
`skyMRT` (clears through the hijacked attachments → clears `inputBuffer.color`
and `emissiveRT.color`); render the unified shader — its two fragment outputs
(`oColor`, `oEmissive`) land **directly** in `inputBuffer` and `emissiveRT`
with zero intermediate blits.

The hijack-attachment trick is load-bearing for smooth frames on TBDR GPUs
(Apple Silicon) alongside screen-space effects like n8ao: per-frame
full-resolution color blits between render targets force tile-memory
resolves, and the resulting sync barriers can hitch downstream passes that
share `inputBuffer` even though GPU/CPU usage both appear low.

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

## Public API

`SkyStackParams` takes a flat list of layers plus an optional background:

```ts
new SkyStack(viz, {
  horizonOffset: -0.025,
  horizonBlend: 0.03,
  layers: [
    cloudsLayer({ id: 'cloudsFront', zIndex: 40, ... }),
    buildingsLayer({ id: 'buildings', zIndex: 30, silhouetteColor: 0x..., ... }),
    cloudsLayer({ id: 'cloudsBack', zIndex: 20, ... }),
    starsLayer({ id: 'stars', zIndex: 10, ... }),
    groundLayer({ id: 'ground', zIndex: 5, paintShader: '...', ... }),
  ],
  background: gradientBackground({ stops: [...], bands: [...] }),
}, width, height);
```

Every layer is produced by a factory function that returns a plain `Layer`
(or `BackgroundLayer`) record. Each factory handles its own:

- Uniform creation (suffixed with `_<id>` for per-instance isolation).
- Per-instance GLSL (uniform decls + helper functions, with `$ID` tokens
  substituted at factory time).
- Shared modules keyed for dedup (e.g. `skyFbm` shared across all cloud
  instances).
- Compile-time `#define` contributions, merged across layers (e.g. clouds
  contribute `MAX_HAZE_OCTAVES` with `merge: 'max'`).
- A body string that calls `accumulate(...)` and an optional gate.

Built-in factories: `starsLayer`, `cloudsLayer`, `buildingsLayer`,
`groundLayer`, plus `customLayer` as the user escape hatch. Backgrounds:
`gradientBackground`, `solidBackground`, `customBackground`. The background
slot is optional — omit it for a black sky.

Runtime mutation is by direct uniform poking: hold a reference to the
factory's output and set `.uniforms['uFoo_<id>'].value = ...`.

## Z-ordering

`zIndex` is CSS-like: higher = closer to camera = emitted first. The
compositor sorts layers by zIndex descending, wraps each in the saturation
guard + optional gate, then runs the background last. Any layer that emits
`alpha=1` short-circuits every layer behind it via the `(1 - accumAlpha)`
weighting; the background is typically the thing that finally saturates, but
an opaque layer in front (buildings silhouette, dense clouds, …) will skip
it entirely.

## Cross-layer independence

Layer bodies reference only compositor-scope variables (`dir`, `elev`,
`azimuth`, `cosElev`, `horizonBlend`, `aboveHorizon`) and their own
id-prefixed uniforms / helpers — never another layer's state. This makes any
layer drop-in additive: an aurora or lightning-flash layer just needs to
decide its zIndex and produce a `Layer` from `customLayer(...)`. If the new
layer emits alpha=1, layers behind it auto-skip; if it's alpha-blended,
layers behind auto-attenuate via `(1 - accumAlpha)`.

## Performance notes

The big perf wins from the recent optimization passes:

- Above-horizon `paintGround` early-out (the SDF/paint cost was the largest
  per-fragment expense; ~40-50% of pixels are above horizon).
- `skyMRT` no depth attachment (saves a per-frame depth blit).
- Baked loop counts (driver unrolling).
- Front-to-back saturation early-out (predictive cloud / silhouette skip).
- Direct MRT writes into `inputBuffer` + `emissiveRT` via attachment hijack
  (eliminates two per-frame color blits; critical on TBDR hardware — see
  pipeline placement section above).
- `emissiveRT.depth` shares `stableDepthTarget.depthTexture` directly (wired
  at setup via `SkyStackPass.setEmissiveDepthTexture()`), eliminating the
  per-frame depth blit entirely. Bypass meshes render with `depthWrite=true`
  into the shared texture — that's intentional for EmissiveFogPass's
  per-pixel fog reconstruction. Tradeoff: FinalPass's fog reads the same
  shared texture and so sees mesh depth at bypass-mesh pixels, which gives
  a slight fog error on scene-behind color there — invisible when bypass
  meshes are opaque (the common case) since their emissive composites on
  top in FinalPass.

Future levers (see `gradient-sky-followups.md` for the longer list):

- Gradient → 1D LUT texture (kills the per-fragment Oklab cube-roots).
- Quality tier gating (octave reduction, layer skipping at low quality).
- Per-cloud unrolled `skyFbm` (currently shared, with runtime octave count).

## File map

| File | Role |
|------|------|
| `SkyStack.ts` | Public class. Owns shared uniforms + the `SkyStackPass`. |
| `SkyStackPass.ts` | postprocessing `Pass`. Allocates RTs, runs the draw + blits. |
| `compose.ts` | Assembles the fragment shader from a layer list: sort by zIndex, dedup modules, merge defines, emit bodies under the saturation guard. |
| `types.ts` | Core types: `Layer`, `BackgroundLayer`, `SharedModule`, `DefineContribution`. |
| `uniforms.ts` | Compositor-shared uniform record + constructor. |
| `shaders/skyUnified.prelude.frag` | Prelude: MRT outputs, helpers, accumulator + `accumulate()`. |
| `shaders/skyStack.vert` | Trivial fullscreen quad vertex shader. |
| `layers/_util.ts` | `resolveId` — id substitution helper for per-instance GLSL. |
| `layers/stars.{ts,glsl}` | Star field layer. |
| `layers/clouds.{ts, module.glsl, instance.glsl}` | Cloud band layer. Shared `skyFbm` module + per-instance sampler. |
| `layers/buildings.{ts,glsl}` | Discrete-tower silhouettes + windows. |
| `layers/ground.{ts,glsl}` | Below-horizon virtual ground plane with a user paint shader. |
| `layers/custom.ts` | Escape hatch — plain `Layer` with user-supplied GLSL. |
| `backgrounds/gradient.{ts,glsl}` | Oklab gradient + additive bands backmost layer. |
| `backgrounds/solid.ts` | Flat solid-color backmost layer. |
| `backgrounds/custom.ts` | Escape hatch for custom backgrounds. |
