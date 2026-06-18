// Near-monochrome albedo: dark plastic base, near-black slots, faintly tinted
// seam (the chamfer shading comes from the relief normal, not albedo).
// AA via gpSlotCarve (footprint-driven edge widen + fade-to-mean), so the slot
// lines dissolve to a flat tone at distance/grazing instead of aliasing.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId;
  float d = gpSquareDist(gpProjectUV(pos, vWorldNormal), cl, cellId);
  float aa = max(ctx.aaFootprint, 1e-4);
  vec3 col = GP_BASE_COLOR;
  float sq = 1. - aaStep(GP_SQ, d, aa);  // 1 inside the slot field, 0 outside (AA'd boundary)
  if (sq > 0.) {
    col = mix(col, GP_GROOVE_COLOR, gpSlotCarve(cl, cellId, aa) * sq);
  }
  float b = 0.5 * GP_CELL - d;
  if (b < GP_SEAM_W) {
    vec2 sv = gpSeamVis(b, aa);
    col = mix(col, GP_GROOVE_COLOR, GP_SEAM_TINT * sv.x + GP_HAIR_TINT * sv.y);
  }
  return vec4(col, 1.);
}
