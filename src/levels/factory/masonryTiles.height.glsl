// POM height field; decomposition lives in masonryTiles.common.glsl. Uses the
// lean `evalMasonryCore` (no cellId) since it runs many times per fragment.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  MasonryCore m = evalMasonryCore(pos, normal, max(MASONRY_BEVEL, distance(cameraPosition, vWorldPos) * 0.001));

  return m.bowl * MASONRY_DENT_DEPTH * m.dentZone + MASONRY_GROOVE_DEPTH * m.grooveZone;
}
