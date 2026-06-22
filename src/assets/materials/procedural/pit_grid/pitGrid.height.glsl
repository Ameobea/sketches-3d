// projectedField (L1): engine hoists the projection and hands us the UV. Carve the centered hole;
// flat (0) over the padding, where the marcher terminates on its first sample.
float gridHeight(vec2 uv, float curTimeSeconds) {
  return pgCarveVS(pgHoleSdf(pgCellLocal(uv))).x;
}
