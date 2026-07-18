// grid tier: the engine hands us cell-local coords + the cached per-tile ribbed
// flag. Grooves carve inside ribbed tiles' fields, the joint along tile
// boundaries; flat (0) elsewhere, where the marcher terminates on its first sample.
float gridHeight(GridCtx ctx, RbCell cell) {
  vec2 cl = ctx.cellLocal;
  float d = max(abs(cl.x), abs(cl.y));
  if (d < RB_FIELD) {
    if (!cell.ribbed) {
      return 0.;
    }
    float g = abs(rbGrooveOffset(cl.y));
    return RB_CARVE
         * (1. - smoothstep(RB_HW, RB_HW + RB_WALL, g))
         * (1. - smoothstep(RB_FIELD - RB_END_WALL, RB_FIELD, abs(cl.x)));
  }
  float b = 0.5 * RB_TILE - d;
  return b < RB_SEAM_W ? rbSeamCarve(b) : 0.;
}
