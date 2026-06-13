// Gaps carve only inside the trenches (the rail band at the edge is already
// flat, so the early-out bound is GT_END_OUT, not GT_HALF_W); flat (0)
// elsewhere, where the marcher terminates on its first sample.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 uv = gtProjectUV(pos, normal);
  float dv = abs(gtTrenchOffset(uv));
  if (dv >= GT_END_OUT) {
    return 0.;
  }
  float g = abs(gtGapOffset(uv));
  return GT_CARVE
       * (1. - smoothstep(GT_GAP_HW, GT_GAP_HW + GT_WALL, g))
       * (1. - smoothstep(GT_END_OUT - GT_END_WALL, GT_END_OUT, dv));
}
