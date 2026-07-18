// Seam valley + pillow shoulder along tile boundaries and inside the corner
// pits; flat (0) across the face, where the marcher terminates on its first
// sample. Keep in sync with copperTiles.normal.glsl.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cl, cellId, dir;
  float b = cuDist(patProjectUV(pos, normal), cl, cellId, dir);
  if (b >= CU_EDGE_W) {
    return 0.;
  }
  return cuCarve(b) * reliefAAFade(cuDirAA(dir), CU_BEVEL_W);
}
