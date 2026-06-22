// Closed-form relief normal: carve = GT_CARVE·gap(g)·end(dv) ⇒ ∇carve =
// GT_CARVE·(end·gap'·∇g + gap·end'·∇dv); both gradients are axis-aligned in
// UV. Linearstep walls (cf. grateTrench.height.glsl): each ramp's slope is a
// constant ±1/width inside its band, zero outside. Keep in sync.
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

  float gap = clamp((GT_GAP_HW + GT_WALL - g) / GT_WALL, 0., 1.);
  float dGap = (g > GT_GAP_HW && g < GT_GAP_HW + GT_WALL) ? -sign(gOff) / GT_WALL : 0.;

  float endMask = clamp((GT_END_OUT - dv) / GT_END_WALL, 0., 1.);
  float dEnd = (dv > GT_END_OUT - GT_END_WALL && dv < GT_END_OUT) ? -sign(vOff) / GT_END_WALL : 0.;

  vec2 uDir = GT_ALONG_X ? vec2(1., 0.) : vec2(0., 1.);
  vec2 vDir = GT_ALONG_X ? vec2(0., 1.) : vec2(1., 0.);
  vec2 gradUV = GT_CARVE * (endMask * dGap * uDir + gap * dEnd * vDir);
  gradUV *= reliefAAFade(aa, GT_WALL);
  vec3 gw = depth * domUnproject(gradUV, axis);
  return normalize(N + (gw - dot(gw, N) * N));
}
