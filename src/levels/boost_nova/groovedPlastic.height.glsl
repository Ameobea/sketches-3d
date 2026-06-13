// Slots carve inside the square fields, the seam joint along cell boundaries;
// flat (0) elsewhere. The early-outs keep the per-march-sample cost minimal
// over the majority-flat surface, where the marcher also terminates on its
// first sample.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cl, cellId;
  float d = gpSquareDist(gpProjectUV(pos, normal), cl, cellId);
  if (d < GP_SQ) {
    bool alongX = gpSlotsAlongX(cellId);
    float g = abs(gpSlotOffset(alongX ? cl.y : cl.x));
    float a = abs(alongX ? cl.x : cl.y);
    return GP_CARVE
         * (1. - smoothstep(GP_HW, GP_HW + GP_WALL, g))
         * (1. - smoothstep(GP_END_OUT - GP_END_WALL, GP_END_OUT, a));
  }
  float b = 0.5 * GP_CELL - d;
  return b < GP_SEAM_W ? gpSeamCarve(b) : 0.;
}
