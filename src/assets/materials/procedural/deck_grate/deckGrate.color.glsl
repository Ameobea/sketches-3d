// Albedo: dark base, near-black gaps. `pomMeshUv(pos)` resolves the UV at the marched POM
// hit, so the gap color tracks the carved relief.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float aa = max(ctx.aaFootprint, 1e-4);
  float gap = dgGap(pomMeshUv(pos), aa);
  vec3 col = mix(DG_BASE_COLOR, DG_GAP_COLOR, gap);
  return vec4(col, 1.);
}
