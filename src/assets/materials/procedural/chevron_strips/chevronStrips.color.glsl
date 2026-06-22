// Region albedo from the shared at-hit frame: ridge color alternates by strip index, darker wall,
// near-black floor. Footprint-AA'd bands dissolve to the ridge tone, then the alternating colors
// average to their mean, so the strips stop aliasing at distance. ctx footprint is UV → ×CH_G to w-space.
vec4 gridColor(ChHit h, vec3 baseColor, SceneCtx ctx) {
  float aw = h.aw;
  float aa = max(ctx.aaFootprint, 1e-4) * CH_G;
  vec3 ridge = mix(CH_COLOR_A, CH_COLOR_B, mod(h.idx, 2.));

  vec3 col = CH_FLOOR_COLOR;
  col = mix(col, ridge * CH_WALL_DARKEN, aaStep(CH_FLOOR_HW + CH_BAND_PAD, aw, aa));
  col = mix(col, ridge, aaStep(CH_WALL_END + CH_BAND_PAD, aw, aa));

  col = mix(col, ridge, fadeToMeanFactor(aa, CH_FADE_PERIOD));
  col = mix(col, 0.5 * (CH_COLOR_A + CH_COLOR_B), fadeToMeanFactor(aa, 2. * CH_WPITCH));
  return vec4(col, 1.);
}
