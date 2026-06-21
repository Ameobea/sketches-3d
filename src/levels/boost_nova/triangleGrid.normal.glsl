// Closed-form POM floor normal, replacing the engine's finite-difference taps.
// Surface = base - depth*carved*N with carved = TRI_FLOOR_DEPTH*smoothstep(ed),
// so the outward normal is normalize(N + depth * tangential(∇carved)). The
// edge-distance gradient is the signed family normal (triEdgeDistGrad); chain it
// through smoothstep'(x)=6t(1-t)/w, then lift to world via the dominant axis.
// Mirrors triangleGrid.height.glsl — keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  int axis = domAxis(N);
  vec2 uv = domProject(pos, axis);
  vec2 gradDir;
  float ed = triEdgeDistGrad(uv, gradDir);

  float ts = clamp((ed - TRI_BORDER_END) / TRI_WALL_WIDTH, 0., 1.);
  float dCarved = TRI_FLOOR_DEPTH * 6. * ts * (1. - ts) / TRI_WALL_WIDTH;
  if (dCarved <= 0.) {
    return N; // flat top or flat floor
  }

  vec2 gradUV = dCarved * gradDir;
  gradUV *= reliefAAFade(aa, TRI_WALL_WIDTH);
  vec3 g = depth * domUnproject(gradUV, axis);
  return normalize(N + (g - dot(g, N) * N));
}
