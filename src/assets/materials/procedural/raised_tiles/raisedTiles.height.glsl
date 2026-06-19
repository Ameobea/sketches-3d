// Flat per-tile plateau, joined to a differing neighbour by a narrow midpoint
// wall near the shared edge. Flush (no wall) where neighbour heights match.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cellId, cl, edgeDir;
  float b = rtCellField(rtProjectUV(pos, normal), cellId, cl, edgeDir);
  float cThis = rtTileCarve(cellId);
  if (b >= RT_WALL_W) {
    return cThis;
  }
  return rtWallCarve(b, cThis, rtTileCarve(cellId + edgeDir));
}
