// Closed-form relief normal: carve = ptGroutCarve(b), b = CELL/2 − chebyshev(cl)
// ⇒ ∇carve = carve'(b)·(−∇cheb), axis-aligned in UV. Outward normal =
// normalize(N + tangential(depth·∇carve)). Mirrors poolTiles.height.glsl — keep
// in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 cl;
  float b = ptBoundaryDist(ptProjectUV(pos, N), cl);
  if (b >= PT_EDGE_W) {
    return N;
  }

  float tb = clamp((b - PT_GROUT_HW) / PT_BEVEL_W, 0., 1.);
  float dCarveDb = -6. * PT_DEPTH * tb * (1. - tb) / PT_BEVEL_W;
  vec2 chebDir = abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
  vec2 gradUV = -dCarveDb * chebDir;
  gradUV *= reliefAAFade(aa, PT_BEVEL_W);

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
