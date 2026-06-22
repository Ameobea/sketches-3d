// Closed-form relief normal: per axis, ∇carve = carve'(b)·(−sign(cl))·presence, summed so seam
// crossings tilt diagonally. Mirrors gridHeight's chamfer slope (the along-seam presence gradient
// is dropped — its slope is negligible). Flat over panel interiors and absent segments.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  int axis = domAxis(N);
  SsSeam s = ssEval(domProject(pos, axis));
  if (s.bx >= SS_SEAM_W && s.by >= SS_SEAM_W) {
    return N;
  }
  vec2 gradUV = vec2(0.);
  if (s.bx < SS_SEAM_W) {
    gradUV.x = -ssSeamCarveVS(s.bx).y * sign(s.clx) * s.pv;
  }
  if (s.by < SS_SEAM_W) {
    gradUV.y = -ssSeamCarveVS(s.by).y * sign(s.cly) * s.ph;
  }
  gradUV *= reliefAAFade(aa, SS_SEAM_W); // key to the broad chamfer so the bevel shading persists at grazing/distance
  vec3 gw = depth * domUnproject(gradUV, axis);
  return normalize(N + (gw - dot(gw, N) * N));
}
