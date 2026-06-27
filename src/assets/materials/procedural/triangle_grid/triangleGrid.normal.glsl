// Closed-form POM floor normal from the shared at-hit frame (TriHit), replacing the engine's
// finite-difference taps. carved = TRI_FLOOR_DEPTH*smoothstep(ed); outward normal =
// normalize(N + depth*tangential(∇carved)); the edge-distance gradient is the signed
// nearest-family normal. Mirrors triangleGrid.height.glsl — keep in sync.
vec3 gridNormal(TriHit h, vec3 N, float depth, float aa) {
  vec3 d = h.d;
  vec2 gradDir;
  float ed;
  if (d.x <= d.y && d.x <= d.z) {
    gradDir = h.sgns.x * TRI_N0;
    ed = d.x;
  } else if (d.y <= d.z) {
    gradDir = h.sgns.y * TRI_N1;
    ed = d.y;
  } else {
    gradDir = h.sgns.z * TRI_N2;
    ed = d.z;
  }

  float ts = clamp((ed - TRI_BORDER_END) / TRI_WALL_WIDTH, 0., 1.);
  float dCarved = TRI_FLOOR_DEPTH * 6. * ts * (1. - ts) / TRI_WALL_WIDTH;
  if (dCarved <= 0.) {
    return N; // flat top or flat floor
  }

  vec2 gradUV = dCarved * gradDir;
  gradUV *= reliefAAFade(aa, TRI_WALL_WIDTH);
  vec3 g = depth * domUnproject(gradUV, domAxis(N));
  return normalize(N + (g - dot(g, N) * N));
}
