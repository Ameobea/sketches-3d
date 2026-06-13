// Closed-form relief normal: carve = psSeamCarve(b), b = CELL/2 − chebyshev(cl)
// ⇒ ∇carve = carve'(b)·(−∇cheb), axis-aligned in UV. Outward normal =
// normalize(N + tangential(depth·∇carve)). Mirrors panelSeams.height.glsl —
// keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t) {
  vec2 cl;
  float b = psBoundaryDist(psProjectUV(pos, N), cl);
  if (b >= PS_SEAM_W) {
    return N;
  }

  float tc = b / PS_SEAM_W;
  float dCarveDb = -6. * PS_SEAM_DEPTH * tc * (1. - tc) / PS_SEAM_W;
  float th = clamp((b - PS_HAIR_HW) / PS_HAIR_WALL, 0., 1.);
  dCarveDb -= 6. * PS_HAIR_DEPTH * th * (1. - th) / PS_HAIR_WALL;
  vec2 chebDir = abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
  vec2 gradUV = -dCarveDb * chebDir;

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
