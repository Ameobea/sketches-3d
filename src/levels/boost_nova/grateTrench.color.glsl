// Near-monochrome albedo: dark plastic base, void-black gaps. AA via gtVisCarve
// (footprint-driven edge widen + fade-to-mean), so the slat-gap lines dissolve
// to a flat tone at distance/grazing instead of aliasing.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = domProject(pos, domAxis(vWorldNormal));
  float dv = abs(gtTrenchOffset(uv));
  float aa = max(ctx.aaFootprint, 1e-4);
  vec3 col = GT_BASE_COLOR;
  if (dv < GT_END_OUT + aa) {
    col = mix(col, GT_VOID_COLOR, gtVisCarve(uv, dv, aa));
  }
  return vec4(col, 1.);
}
