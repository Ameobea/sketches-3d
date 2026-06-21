// grid tier: the engine hands us the cell-local coords + the cached cell (slot orientation).
// Slots carve inside the square fields, the seam joint along cell boundaries; flat (0)
// elsewhere, where the marcher also terminates on its first sample.
float gridHeight(GridCtx ctx, GpCell cell) {
  vec2 cl = ctx.cellLocal;
  float d = max(abs(cl.x), abs(cl.y));
  if (d < GP_SQ) {
    float g = abs(gpSlotOffset(cell.alongX ? cl.y : cl.x));
    float a = abs(cell.alongX ? cl.x : cl.y);
    return GP_CARVE
         * (1. - smoothstep(GP_HW, GP_HW + GP_WALL, g))
         * (1. - smoothstep(GP_END_OUT - GP_END_WALL, GP_END_OUT, a));
  }
  float b = 0.5 * GP_CELL - d;
  return b < GP_SEAM_W ? gpSeamCarve(b) : 0.;
}
