// Tile tops flat (0), gutters at SE_FLOOR, beveled transition between. Both
// flats early-out before the gradient-normalization work; the bounds are exact
// because |∇f| ≤ 1 (s never has smaller magnitude than f − SE_R).
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 cl = seCellLocal(seProjectUV(pos, normal));
  float f = seRadius(cl);
  if (f <= SE_R - SE_RND * SE_BEV) {
    return 0.;
  }
  if (f - SE_R >= (1. + SE_RND) * SE_BEV) {
    return SE_FLOOR;
  }
  vec2 dir;
  float s = seOutlineDist(cl, f, dir);
  return SE_FLOOR * seRamp(s / SE_BEV, SE_RND).x;
}
