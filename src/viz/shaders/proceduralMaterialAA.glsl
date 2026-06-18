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
