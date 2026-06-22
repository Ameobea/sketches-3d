// Fake AO: darken indirect light inside the pits (→ recessed read), direct kept so wall highlights
// survive. Coverage matches the color slot, dissolving to the duty mean at distance.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float d = pgHoleSdf(pgCellLocal(domProject(pos, domAxis(vWorldNormal))));
  float aa = max(ctx.aaFootprint, 1e-4);
  float cov = fadeToMean(1. - aaStep(0., d, aa), PG_DUTY, aa, PG_FADE_PERIOD);
  return vec2(1., mix(1., PG_AO, cov));
}
