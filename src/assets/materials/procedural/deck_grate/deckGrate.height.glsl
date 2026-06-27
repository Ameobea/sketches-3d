// Carved depth in [0,1] for tangent-space POM: 0 = slat top, 1 = full pom.depth in the gaps.
// `pomMeshUv(pos)` projects the marched hit into the swept tangent frame, so the grooves
// recess along U = arc length and track the arch.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  return dgGap(pomMeshUv(pos), 0.0);
}
