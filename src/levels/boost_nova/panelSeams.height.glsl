// Seam joint along cell boundaries; flat (0) elsewhere, where the marcher
// terminates on its first sample.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cl;
  float b = psBoundaryDist(psProjectUV(pos, normal), cl);
  return b < PS_SEAM_W ? psSeamCarve(b) : 0.;
}
