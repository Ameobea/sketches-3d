// Mild AO over the chamfer valley + spec kill in the gap line, scaled by presence; full light over
// panel interiors and absent segments.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  SsSeam s = ssEval(domProject(pos, domAxis(vWorldNormal)));
  float aa = max(ctx.aaFootprint, 1e-4);
  float direct = 1., indirect = 1.;
  if (s.bx < SS_SEAM_W) {
    vec2 sv = ssSeamVis(s.bx, aa);
    direct = min(direct, mix(1., SS_DIRECT_HAIR, sv.y * s.pv));
    indirect = min(indirect, mix(1., SS_AO_SEAM, sv.x * s.pv));
  }
  if (s.by < SS_SEAM_W) {
    vec2 sv = ssSeamVis(s.by, aa);
    direct = min(direct, mix(1., SS_DIRECT_HAIR, sv.y * s.ph));
    indirect = min(indirect, mix(1., SS_AO_SEAM, sv.x * s.ph));
  }
  return vec2(direct, indirect);
}
