// grid tier: the engine hands us cell-local coords + the cached cell (this tile's carve).
// Flat per-tile plateau, joined to a differing neighbour by a narrow midpoint wall near the
// shared edge (the neighbour's carve is a bounded lookup, computed only at edges). Flush where
// neighbour heights match.
float gridHeight(GridCtx ctx, RtCell cell) {
  vec2 cl = ctx.cellLocal;
  float b = 0.5 * RT_CELL - max(abs(cl.x), abs(cl.y));
  if (b >= RT_WALL_W) {
    return cell.carve;
  }
  vec2 edgeDir = abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
  return rtWallCarve(b, cell.carve, gridComputeCell(ctx.cellId + edgeDir).carve);
}
