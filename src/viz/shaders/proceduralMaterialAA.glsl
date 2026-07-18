// Footprint-driven analytic AA helpers for procedural materials. `aa` is the
// world-space pixel-footprint half-width — pass SceneCtx.aaFootprint, which adds
// the 1/NdotV grazing stretch. These stand in for screen-space derivatives
// (unusable on the POM `discard` path) and let high-frequency detail dissolve to
// a flat tone at distance/grazing instead of aliasing. Included whenever a
// material supplies any GLSL shader snippet.
//
// Tuning is two-level: these global shape constants set the fade *bands* (as
// multiples of the per-call period/width), while each material passes its own
// feature `period`/`width` — the absolute scale — to the functions below. Raise
// a band (or a material's period) to keep detail crisp longer; lower it to
// dissolve sooner. Glancing surfaces (floors) have a large `aa`, so the fades
// engage close to the camera there — push these up if a floor fades too soon.
const float AA_EDGE_WIDTH = 1.0;  // hard-edge softening half-width, ×footprint
const float AA_FADE_LO    = 0.5;  // fade-to-mean: begins at LO×period ...
const float AA_FADE_HI    = 1.0;  // ... and completes at HI×period
const float AA_RELIEF_LO  = 0.4;  // relief-normal flatten: begins at LO×featureWidth ...
const float AA_RELIEF_HI  = 1.2;  // ... and completes at HI×featureWidth

// Pixel footprints filled once in main() — before the POM march/discards, so they stay
// valid on paths where later derivatives aren't and reachable from the ctx-less POM
// height/normal slots. `aaWorldFootprint` is the world-space half-width (the same value
// SceneCtx.aaFootprint carries); `aaUvFootprint` is the per-axis screen-derivative
// footprint of vUv — UV-keyed materials multiply it by their pattern scale for
// anisotropic pattern-space AA.
float aaWorldFootprint = 0.;
vec2 aaUvFootprint = vec2(0.);

// World-space directions of increasing vUv.x / vUv.y (unit, cotangent-frame from
// screen derivatives), filled with the footprints. UV-keyed materials map their
// pattern-space relief gradients through these instead of the dominant world axis
// of N — that mapping switches basis mid-surface on curved geometry, flipping the
// relief direction along a visible line.
vec3 uvFrameT = vec3(1., 0., 0.);
vec3 uvFrameB = vec3(0., 1., 0.);

// AA'd hard step at `edge`: a sharp region boundary softened over the footprint.
float aaStep(float edge, float x, float aa) {
  return smoothstep(edge - AA_EDGE_WIDTH * aa, edge + AA_EDGE_WIDTH * aa, x);
}

// Lerp factor [0,1] for collapsing detail of scale `period` to its mean as the
// footprint grows — use directly to fade a vec3 (e.g. a region color toward its
// mean). `fadeToMean` is the scalar-coverage convenience wrapper.
float fadeToMeanFactor(float aa, float period) {
  return smoothstep(AA_FADE_LO * period, AA_FADE_HI * period, aa);
}

// Converge a periodic coverage value to its area-average `mean` as the footprint
// reaches the pattern `period` — the analytic mip that edge-widening (a softened
// nearest edge) can't provide, so detail dissolves to a flat tone vs shimmering.
float fadeToMean(float coverage, float mean, float aa, float period) {
  return mix(coverage, mean, fadeToMeanFactor(aa, period));
}

// Coverage of one dark slot/line in a pattern repeating every `period`: a dark
// floor of half-width `floorHW` with `wallW`-wide ramps, footprint-widened, then
// dissolved to its duty-cycle mean past the period. `offset` = |distance to the
// nearest slot centerline|. Mean is auto-derived, so callers need no constant.
float aaSlot(float offset, float period, float floorHW, float wallW, float aa) {
  float mid = floorHW + 0.5 * wallW;
  float w = max(0.5 * wallW, aa);
  float local = 1. - smoothstep(mid - w, mid + w, offset);
  float duty = (2. * floorHW + wallW) / period;
  return fadeToMean(local, duty, aa, period);
}

// Fade an isolated thin feature (a lone seam/gap line, spacing >> width) out to
// zero as it goes sub-pixel — its area-mean is ~0, so dissolving it kills the
// thin-line shimmer. `featureWidth` is the feature's full width.
float aaThinFeature(float coverage, float aa, float featureWidth) {
  return fadeToMean(coverage, 0., aa, featureWidth);
}

// Relief-AA fade for closed-form POM normals: 1 = full relief, 0 = collapse to
// the flat base normal as the footprint passes the relief's narrowest wall.
float reliefAAFade(float aa, float featureWidth) {
  return 1. - smoothstep(AA_RELIEF_LO * featureWidth, AA_RELIEF_HI * featureWidth, aa);
}

// Coverage of one isolated dark line: a floor of half-width `floorHW` with
// `wallW`-wide ramps, footprint-widened, then dissolved to zero as `fadeWidth`
// goes sub-pixel. `offset` = |distance to the line's centerline|. The repeating-
// pattern analog is aaSlot. Key `fadeWidth` to the full joint width (not the
// line's own tiny width) so the line persists as long as the joint resolves.
float aaLine(float offset, float floorHW, float wallW, float fadeWidth, float aa) {
  float mid = floorHW + 0.5 * wallW;
  float w = max(0.5 * wallW, aa);
  return aaThinFeature(1. - smoothstep(mid - w, mid + w, offset), aa, fadeWidth);
}

// As above with the fade self-keyed to the line's own full width — the common case.
float aaLine(float offset, float floorHW, float wallW, float aa) {
  return aaLine(offset, floorHW, wallW, 2. * floorHW + wallW, aa);
}

// Dominant-axis projection (Y→xz, X→zy, Z→xy) shared by the world-space
// materials and the POM marcher; `domUnproject` maps in-plane (depth-free)
// vectors — e.g. a pattern-space gradient — back to world.
int domAxis(vec3 n) {
  vec3 a = abs(n);
  if (a.y >= a.x && a.y >= a.z) { return 1; }
  if (a.x >= a.z) { return 0; }
  return 2;
}

vec2 domProject(vec3 p, int axis) {
  if (axis == 1) { return p.xz; }
  if (axis == 0) { return vec2(p.z, p.y); }
  return vec2(p.x, p.y);
}

vec3 domUnproject(vec2 v, int axis) {
  if (axis == 1) { return vec3(v.x, 0., v.y); }
  if (axis == 0) { return vec3(0., v.y, v.x); }
  return vec3(v.x, v.y, 0.);
}

// smoothstep paired with its derivative d/dx, so a profile's height and normal share one
// definition instead of hand-transcribing the 6t(1-t) slope. .x = value, .y = slope.
vec2 smoothstepVS(float e0, float e1, float x) {
  float t = clamp((x - e0) / (e1 - e0), 0., 1.);
  return vec2(t * t * (3. - 2. * t), 6. * t * (1. - t) / (e1 - e0));
}

// ---- Anisotropy-aware directional footprints --------------------------------
// `aaWorldFootprint`/`ctx.aaFootprint` carry the footprint ellipse's MAJOR axis,
// so an isolated line viewed along its own length over-fades at grazing — the
// 1/NdotV stretch runs along the line while the across-line footprint stays
// ≈ 1px. These instead project the ellipse (minor ≈ unitsPerPx, major stretched
// 1/NdotV along the tangential view direction) onto a chosen direction: pass the
// across-feature direction so a line's darkening survives until its true width
// goes sub-pixel. Scale the result to tune fade reach (× 0.4 reads well for tile
// joints — a little shimmer traded for detail persisting further out).
float aaDirFootprint(vec3 d) {
  vec3 n = normalize(vWorldNormal);
  vec3 v = vWorldPos - cameraPosition;
  float dist = max(length(v), 1e-4);
  v /= dist;
  float q = dot(d, v - dot(v, n) * n) / max(abs(dot(n, v)), 0.1);
  return dist * unitsPerPxScale * sqrt(1. + q * q);
}

// As aaDirFootprint for a pattern-space direction under the dominant-axis
// projection; UV-mode materials should map their direction through
// uvFrameT/uvFrameB and call aaDirFootprint directly.
float aaPatternDirFootprint(vec2 dp) {
  return aaDirFootprint(domUnproject(dp, domAxis(vWorldNormal)));
}
