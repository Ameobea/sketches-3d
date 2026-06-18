// The gaps read as holes into a dark void: strong AO + direct kill at full
// carve. The slat tops and rails stay fully lit.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = gtProjectUV(pos, vWorldNormal);
  float dv = abs(gtTrenchOffset(uv));
  float aa = max(ctx.aaFootprint, 1e-4);
  if (dv >= GT_END_OUT + aa) {
    return vec2(1.);
  }
  float carve = gtVisCarve(uv, dv, aa);
  return vec2(mix(1., GT_DIRECT_VOID, carve), mix(1., GT_AO_VOID, carve));
}
