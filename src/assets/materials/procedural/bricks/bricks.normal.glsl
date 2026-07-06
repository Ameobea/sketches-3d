// Closed-form relief normal: carve = brJointCarve(b), b = min axis boundary dist
// ⇒ ∇carve = carve'(b)·(−∇cheb), axis-aligned in UV. Outward normal =
// normalize(N + tangential(depth·∇carve)). Mirrors bricks.height.glsl — keep in
// sync (same per-axis relief fade).
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 brickId, cl;
  vec2 bd = brCellField(brProjectUV(pos, N), brickId, cl);
  float b = min(bd.x, bd.y);
  if (b >= BR_EDGE_W) {
    return N;
  }

  float tb = clamp((b - BR_JOINT_HW) / BR_BEVEL_W, 0., 1.);
  float dCarveDb = -6. * BR_DEPTH * tb * (1. - tb) / BR_BEVEL_W;
  vec2 chebDir = bd.x <= bd.y ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
  vec2 gradUV = -dCarveDb * chebDir * brReliefFade(bd);

#if BR_UV_MODE == 1
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
