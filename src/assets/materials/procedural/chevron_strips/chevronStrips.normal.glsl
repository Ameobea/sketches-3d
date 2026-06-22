// Closed-form relief normal from the shared at-hit frame: ∇carve = (dcarve/daw)·∇aw, ∇aw = off·∇w
// (un-normalized, |∇w| = CH_G). Outward normal = normalize(N + tangential(depth·∇carve)). Slope
// shared with the height via chCarveVS. cf. triangleGrid.normal.glsl.
vec3 gridNormal(ChHit h, vec3 N, float depth, float aa) {
  float dca = chCarveVS(h.aw).y;
  if (dca == 0.) {
    return N; // flat top or flat floor
  }
  vec2 gradUV = dca * h.off * chGradW(h.su);
  gradUV *= reliefAAFade(aa * CH_G, CH_WALL_W);
  vec3 g = depth * domUnproject(gradUV, domAxis(N));
  return normalize(N + (g - dot(g, N) * N));
}
