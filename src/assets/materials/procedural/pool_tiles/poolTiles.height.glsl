// Grout valley + rounded rim along cell boundaries; flat (0) across the tile face,
// where the marcher terminates on its first sample. The carve amplitude fades with
// the carving axis's footprint so sub-pixel relief flattens instead of aliasing —
// keep in sync with poolTiles.normal.glsl.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cl;
  vec2 bd = ptBoundaryDist(ptProjectUV(pos, normal), cl);
  float b = min(bd.x, bd.y);
  if (b >= PT_EDGE_W) {
    return 0.;
  }
  return ptGroutCarve(b) * ptReliefFade(bd);
}
