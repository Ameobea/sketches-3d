// projectedField (L1): the engine hoists the projection and hands us the UV. Seam joint along
// cell boundaries; flat (0) elsewhere, where the marcher terminates on its first sample.
float gridHeight(vec2 uv, float curTimeSeconds) {
  vec2 cl;
  float b = psBoundaryDist(uv, cl);
  return b < PS_SEAM_W ? psSeamCarveVS(b).x : 0.;
}
