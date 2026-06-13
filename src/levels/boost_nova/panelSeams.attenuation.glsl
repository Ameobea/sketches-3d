// Spec is killed only inside the gap line — the chamfers keep their glints,
// which is what sells the joint. Mild AO over the seam valley.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  float b = psBoundaryDist(psProjectUV(pos, vWorldNormal), cl);
  if (b >= PS_SEAM_W) {
    return vec2(1.);
  }
  vec2 sv = psSeamVis(b, max(ctx.unitsPerPx, 1e-4));
  return vec2(mix(1., PS_DIRECT_HAIR, sv.y), mix(1., PS_AO_SEAM, sv.x));
}
