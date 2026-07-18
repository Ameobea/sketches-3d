// Painted albedo darkening into the grooves, overlaid with wear streaks running
// along the corrugation, dissolved with footprint.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 p = patProjectUV(pos, vWorldNormal);
  vec2 aa = patAA();
  vec3 col = mix(CM_COLOR, CM_COLOR_DEEP, cmCov(p.x, aa.x));
  col *= 1. + 2. * CM_STREAK_AMP * cmStreakSignal(p, aa);
  return vec4(max(col, 0.), 1.);
}
