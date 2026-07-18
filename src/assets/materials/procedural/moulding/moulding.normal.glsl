// Relief normal from the analytic carve gradient, mapped to world via the UV
// frame (UV mode) or the dominant axis of N (world mode).
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 gradUV = mdCarveGrad(patProjectUV(pos, N));

  vec3 gw = depth * patGradToWorld(gradUV, N);
  return normalize(N + (gw - dot(gw, N) * N));
}
