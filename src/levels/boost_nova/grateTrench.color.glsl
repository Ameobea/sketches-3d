// Near-monochrome albedo: dark plastic base, void-black gaps. Oversampled
// (antialiasColorShader); edges widen to unitsPerPx so the gap lines average
// toward coverage at distance.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = gtProjectUV(pos, vWorldNormal);
  float dv = abs(gtTrenchOffset(uv));
  vec3 col = GT_BASE_COLOR;
  if (dv < GT_END_OUT) {
    col = mix(col, GT_VOID_COLOR, gtVisCarve(uv, dv, max(ctx.unitsPerPx, 1e-4)));
  }
  return vec4(col, 1.);
}
