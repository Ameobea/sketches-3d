// Matte concrete with a touch of fbm variation; the seam line reads slightly
// rougher/porous.
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = rtProjectUV(pos, vWorldNormal);
  vec2 cellId, cl, edgeDir;
  float b = rtCellField(uv, cellId, cl, edgeDir);
  float aa = max(ctx.aaFootprint, 1e-4);
  float r = RT_BASE_ROUGH + 0.06 * (fbm(uv * 2.0) - 0.5);
  float seam = aaThinFeature(1.0 - smoothstep(RT_SEAM_HW, RT_SEAM_HW + max(aa, 0.006), b), aa, RT_FADE_PERIOD);
  return clamp(mix(r, 0.97, seam), 0.0, 1.0);
}
