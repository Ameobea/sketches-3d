// Grout valley + rounded rim along cell boundaries; flat (0) across the tile
// face, where the marcher terminates on its first sample.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cl;
  float b = ptBoundaryDist(ptProjectUV(pos, normal), cl);
  return b < PT_EDGE_W ? ptGroutCarve(b) : 0.;
}
