// Near-monochrome albedo: dark base, faintly tinted present seams (chamfer + darker gap line),
// scaled by presence and AA-dissolved as the thin lines go sub-pixel.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = domProject(pos, domAxis(vWorldNormal));
  SsSeam s = ssEval(uv);
  float aa = max(ctx.aaFootprint, 1e-4);
  vec3 col = ssDeckTint(SS_BASE_COLOR, uv, aa);
  if (s.bx < SS_SEAM_W) {
    vec2 sv = ssSeamVis(s.bx, aa);
    col = mix(col, SS_DARK_COLOR, (SS_SEAM_TINT * sv.x + SS_HAIR_TINT * sv.y) * s.pv);
  }
  if (s.by < SS_SEAM_W) {
    vec2 sv = ssSeamVis(s.by, aa);
    col = mix(col, SS_DARK_COLOR, (SS_SEAM_TINT * sv.x + SS_HAIR_TINT * sv.y) * s.ph);
  }
  return vec4(col, 1.);
}
