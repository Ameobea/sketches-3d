// POM height field. The geometric decomposition lives in
// masonryTiles.common.glsl (`evalMasonry`); this only applies depth weights.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  Masonry m = evalMasonry(pos, normal, distance(cameraPosition, vWorldPos));

  const float GROOVE_DEPTH = 1.;
  const float DENT_DEPTH = 0.4;
  return m.bowl * DENT_DEPTH * m.dentZone + GROOVE_DEPTH * m.grooveZone;
}
