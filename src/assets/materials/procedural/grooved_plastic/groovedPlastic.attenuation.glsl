// Carve-scaled dimming: direct (kills specular punch-through in the slots and
// the seam gap — but NOT the seam chamfers, whose spec glints sell the joint)
// + indirect (fake AO). Too shallow everywhere for a cast shadow to read.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId;
  float d = gpSquareDist(gpProjectUV(pos, vWorldNormal), cl, cellId);
  float aa = max(ctx.aaFootprint, 1e-4);
  vec2 atten = vec2(1.);
  float sq = 1. - aaStep(GP_SQ, d, aa);  // 1 inside the slot field, 0 outside (AA'd boundary)
  if (sq > 0.) {
    float carve = gpSlotCarve(cl, cellId, aa) * sq;
    atten = vec2(mix(1., GP_DIRECT_GROOVE, carve), mix(1., GP_AO_GROOVE, carve));
  }
  float b = 0.5 * GP_CELL - d;
  if (b < GP_SEAM_W) {
    vec2 sv = gpSeamVis(b, aa);
    atten *= vec2(mix(1., GP_DIRECT_HAIR, sv.y), mix(1., GP_AO_SEAM, sv.x));
  }
  return atten;
}
