// projectedField (L1): seam joints along present cell-boundary segments (depth scaled by presence);
// flat (0) over the panel interiors, where the marcher terminates on its first sample.
float gridHeight(vec2 uv, float curTimeSeconds) {
  SsSeam s = ssEval(uv);
  float carve = 0.;
  if (s.bx < SS_SEAM_W) {
    carve = max(carve, ssSeamCarveVS(s.bx).x * s.pv);
  }
  if (s.by < SS_SEAM_W) {
    carve = max(carve, ssSeamCarveVS(s.by).x * s.ph);
  }
  return carve;
}
