// Closed-form relief normal: carve = GT_CARVE·gap(g)·end(dv) ⇒ ∇carve =
// GT_CARVE·(end·gap'·∇g + gap·end'·∇dv); both gradients are axis-aligned in
// UV. Outward normal = normalize(N + tangential(depth·∇carve)). Mirrors
// grateTrench.height.glsl — keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  int axis = domAxis(N);
  vec2 uv = domProject(pos, axis);
  float vOff = gtTrenchOffset(uv);
  float dv = abs(vOff);
  if (dv >= GT_END_OUT) {
    return N;
  }

  float gOff = gtGapOffset(uv);
  float g = abs(gOff);

  float tg = clamp((g - GT_GAP_HW) / GT_WALL, 0., 1.);
  float gap = 1. - tg * tg * (3. - 2. * tg);
  float dGap = -6. * tg * (1. - tg) / GT_WALL * sign(gOff);

  float te = clamp((dv - GT_END_OUT + GT_END_WALL) / GT_END_WALL, 0., 1.);
  float endMask = 1. - te * te * (3. - 2. * te);
  float dEnd = -6. * te * (1. - te) / GT_END_WALL * sign(vOff);

  vec2 uDir = GT_ALONG_X ? vec2(1., 0.) : vec2(0., 1.);
  vec2 vDir = GT_ALONG_X ? vec2(0., 1.) : vec2(1., 0.);
  vec2 gradUV = GT_CARVE * (endMask * dGap * uDir + gap * dEnd * vDir);
  gradUV *= reliefAAFade(aa, GT_WALL);
  vec3 gw = depth * domUnproject(gradUV, axis);
  return normalize(N + (gw - dot(gw, N) * N));
}
