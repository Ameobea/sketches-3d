// Closed-form relief normal: carve = SE_FLOOR·ramp(s/SE_BEV) ⇒ ∇carve =
// SE_FLOOR·ramp'·dir/SE_BEV, with dir the normalized superellipse gradient
// (∇s up to curvature terms — exact enough at these slopes). Outward normal =
// normalize(N + tangential(depth·∇carve)). Mirrors superellipseTiles.height.glsl
// — keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 cl = seCellLocal(seProjectUV(pos, N));
  float f = seRadius(cl);
  if (f <= SE_R - SE_RND * SE_BEV || f - SE_R >= (1. + SE_RND) * SE_BEV) {
    return N;
  }
  vec2 dir;
  float s = seOutlineDist(cl, f, dir);
  vec2 gradUV = SE_FLOOR * seRamp(s / SE_BEV, SE_RND).y / SE_BEV * dir;
  gradUV *= reliefAAFade(aa, SE_BEV);

  vec3 na = abs(N);
  vec3 gradW;
  if (na.y >= na.x && na.y >= na.z) {
    gradW = vec3(gradUV.x, 0., gradUV.y);
  } else if (na.x >= na.z) {
    gradW = vec3(0., gradUV.y, gradUV.x);
  } else {
    gradW = vec3(gradUV.x, gradUV.y, 0.);
  }

  vec3 gw = depth * gradW;
  return normalize(N + (gw - dot(gw, N) * N));
}
