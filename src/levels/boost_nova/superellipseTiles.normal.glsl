// Closed-form relief normal: carve = SE_FLOOR·ramp(s/SE_BEV) ⇒ ∇carve =
// SE_FLOOR·ramp'·dir/SE_BEV, with dir the normalized superellipse gradient
// (∇s up to curvature terms — exact enough at these slopes). Outward normal =
// normalize(N + tangential(depth·∇carve)). Mirrors superellipseTiles.height.glsl
// — keep in sync.
vec3 gridNormal(SeHit h, vec3 N, float depth, float aa) {
  if (h.f <= SE_R - SE_RND * SE_BEV || h.f - SE_R >= (1. + SE_RND) * SE_BEV) {
    return N;
  }
  vec2 gradUV = SE_FLOOR * seRamp(h.s / SE_BEV, SE_RND).y / SE_BEV * h.dir;
  gradUV *= reliefAAFade(aa, SE_BEV);
  vec3 gw = depth * domUnproject(gradUV, domAxis(N));
  return normalize(N + (gw - dot(gw, N) * N));
}
