// Closed-form relief normal. Grooves: carve = RB_CARVE·groove(g)·end(x) ⇒
// ∇carve via smoothstepVS. Joint: carve = rbSeamCarve(b), b = TILE/2 − chebyshev
// ⇒ ∇b = −chebDir. Outward normal = normalize(N + tangential(depth·∇carve)).
// Mirrors ribbedTiles.height.glsl — keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  int axis = domAxis(N);
  vec2 cl, cellId;
  float d = rbCellDist(domProject(pos, axis), cl, cellId);

  vec2 gradUV;
  if (d < RB_FIELD) {
    if (!rbRibbed(cellId)) {
      return N;
    }
    float off = rbGrooveOffset(cl.y);
    vec2 sw = smoothstepVS(RB_HW, RB_HW + RB_WALL, abs(off));
    vec2 ew = smoothstepVS(RB_FIELD - RB_END_WALL, RB_FIELD, abs(cl.x));
    gradUV = -RB_CARVE * vec2((1. - sw.x) * ew.y * sign(cl.x), (1. - ew.x) * sw.y * sign(off));
    gradUV *= reliefAAFade(aa, RB_WALL);
  } else {
    float b = 0.5 * RB_TILE - d;
    if (b >= RB_SEAM_W) {
      return N;
    }
    float dCarveDb = -RB_SEAM_DEPTH * smoothstepVS(RB_SEAM_HW, RB_SEAM_W, b).y;
    gradUV = -dCarveDb * rbChebDir(cl);
    gradUV *= reliefAAFade(aa, RB_SEAM_WALL);
  }

  vec3 gw = depth * domUnproject(gradUV, axis);
  return normalize(N + (gw - dot(gw, N) * N));
}
