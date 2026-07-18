// Blue-gray concrete with per-tile value jitter; grooves and joints darken
// toward the carve. AA'd via aaSlot/aaThinFeature so the lines dissolve to a
// flat tone at distance/grazing instead of aliasing.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId;
  float d = rbCellDist(domProject(pos, domAxis(vWorldNormal)), cl, cellId);
  float aa = max(ctx.aaFootprint, 1e-4);
  vec3 col = RB_BASE_COLOR * (1. + RB_TINT_AMP * (2. * hash(cellId + 7.3) - 1.));
  float sq = 1. - aaStep(RB_FIELD, d, aa);
  if (sq > 0. && rbRibbed(cellId)) {
    col *= mix(1., RB_GROOVE_DARKEN, rbGrooveCarve(cl, aa) * sq);
  }
  float b = 0.5 * RB_TILE - d;
  if (b < RB_SEAM_W) {
    col = mix(col, RB_SEAM_COLOR, rbSeamVis(b, rbSeamAA(cl)));
  }
  return vec4(col, 1.);
}
