// Mortar valley + rounded rim along running-bond boundaries; flat (0) across the
// block face, where the marcher terminates on its first sample. Carve amplitude
// fades with the carving axis's footprint — keep in sync with bricks.normal.glsl.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 brickId, cl;
  vec2 bd = brCellField(patProjectUV(pos, normal), brickId, cl);
  float b = min(bd.x, bd.y);
  if (b >= BR_EDGE_W) {
    return 0.;
  }
  return brJointCarve(b) * brReliefFade(bd);
}
