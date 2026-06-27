// Closed-form relief normal: carve(b), b = CELL/2 − chebyshev(cl) ⇒ ∇carve =
// carve'(b)·(−∇cheb), axis-aligned in UV. Outward normal = normalize(N +
// tangential(depth·∇carve)). Slope shared with the height via psSeamCarveVS.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  int axis = domAxis(N);
  vec2 cl;
  float b = psBoundaryDist(domProject(pos, axis), cl);
  if (b >= PS_SEAM_W) {
    return N;
  }

  float dCarveDb = psSeamCarveVS(b).y;
  vec2 chebDir = abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
  vec2 gradUV = -dCarveDb * chebDir * reliefAAFade(aa, PS_HAIR_WALL);

  vec3 gw = depth * domUnproject(gradUV, axis);
  return normalize(N + (gw - dot(gw, N) * N));
}
