// Carve-scaled dimming: indirect (fake AO in the grooves/joints) + direct
// (kills specular punch-through). Too shallow for a cast shadow to read.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId;
  float d = rbCellDist(domProject(pos, domAxis(vWorldNormal)), cl, cellId);
  float aa = max(ctx.aaFootprint, 1e-4);
  vec2 atten = vec2(1.);
  float sq = 1. - aaStep(RB_FIELD, d, aa);
  if (sq > 0. && rbRibbed(cellId)) {
    float carve = rbGrooveCarve(cl, aa) * sq;
    atten = vec2(mix(1., RB_DIRECT_GROOVE, carve), mix(1., RB_AO_GROOVE, carve));
  }
  float b = 0.5 * RB_TILE - d;
  if (b < RB_SEAM_W) {
    float sv = rbSeamVis(b, rbSeamAA(cl));
    atten *= vec2(mix(1., RB_DIRECT_SEAM, sv), mix(1., RB_AO_SEAM, sv));
  }
  return atten;
}
