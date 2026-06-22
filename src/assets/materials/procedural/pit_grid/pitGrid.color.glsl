// Subdued two-tone albedo: flat top vs darker pit interior, blended across the rim and dissolved
// to the duty-cycle mean (PG_DUTY) as the footprint outgrows the cell — so distant fields read as
// a flat tone instead of shimmering. Wall shading itself comes from the relief normal, not albedo.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float d = pgHoleSdf(pgCellLocal(domProject(pos, domAxis(vWorldNormal))));
  float aa = max(ctx.aaFootprint, 1e-4);
  float cov = fadeToMean(1. - aaStep(0., d, aa), PG_DUTY, aa, PG_FADE_PERIOD);
  return vec4(mix(PG_TOP_COLOR, PG_PIT_COLOR, cov), 1.);
}
