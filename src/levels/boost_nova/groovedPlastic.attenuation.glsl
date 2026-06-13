// Carve-scaled dimming: direct (kills specular punch-through in the slots and
// the seam gap — but NOT the seam chamfers, whose spec glints sell the joint)
// + indirect (fake AO). Too shallow everywhere for a cast shadow to read.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId;
  float d = gpSquareDist(gpProjectUV(pos, vWorldNormal), cl, cellId);
  float aa = max(ctx.unitsPerPx, 1e-4);
  if (d < GP_SQ) {
    float carve = gpSlotCarve(cl, cellId, aa);
    return vec2(mix(1., GP_DIRECT_GROOVE, carve), mix(1., GP_AO_GROOVE, carve));
  }
  float b = 0.5 * GP_CELL - d;
  if (b >= GP_SEAM_W) {
    return vec2(1.);
  }
  vec2 sv = gpSeamVis(b, aa);
  return vec2(mix(1., GP_DIRECT_HAIR, sv.y), mix(1., GP_AO_SEAM, sv.x));
}
