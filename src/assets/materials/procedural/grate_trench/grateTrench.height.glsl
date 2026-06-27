// Gaps carve only inside the trenches (the rail band at the edge is already
// flat, so the early-out bound is GT_END_OUT, not GT_HALF_W); flat (0)
// elsewhere, where the marcher terminates on its first sample.
// linearstep (not smoothstep) walls: carve = gap(g)·end(dv) becomes piecewise
// quadratic along a view ray, which `gridAnalyticHit` solves in closed form
// (Tier-A). The tradeoff is C0 ramp ends instead of smoothstep's C1 round.
float gridHeight(vec2 uv, float t) {
  float dv = abs(gtTrenchOffset(uv));
  if (dv >= GT_END_OUT) {
    return 0.;
  }
  float g = abs(gtGapOffset(uv));
  return GT_CARVE
       * clamp((GT_GAP_HW + GT_WALL - g) / GT_WALL, 0., 1.)
       * clamp((GT_END_OUT - dv) / GT_END_WALL, 0., 1.);
}
