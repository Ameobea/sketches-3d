// Relief normal from the analytic carve gradient, mapped to world via the UV
// frame (UV mode) or the dominant axis of N (world mode).
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 gradUV = mdCarveGrad(mdProjectUV(pos, N));

#if MD_UV_MODE == 1
  vec3 gw = depth * (gradUV.x * uvFrameT + gradUV.y * uvFrameB);
#else
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
#endif
  return normalize(N + (gw - dot(gw, N) * N));
}
