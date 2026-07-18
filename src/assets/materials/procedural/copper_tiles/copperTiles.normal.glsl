// Closed-form relief normal: carve = cuCarve(b), ∇b = −dir ⇒ ∇carve =
// −carve′(b)·dir. Mirrors copperTiles.height.glsl — keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 cl, cellId, dir;
  float b = cuDist(patProjectUV(pos, N), cl, cellId, dir);
  if (b >= CU_EDGE_W) {
    return N;
  }
  float tb = clamp((b - CU_SEAM_HW) / CU_BEVEL_W, 0., 1.);
  float dCarveDb = -6. * CU_DEPTH * tb * (1. - tb) / CU_BEVEL_W;
  vec2 gradUV = -dCarveDb * dir * reliefAAFade(cuDirAA(dir), CU_BEVEL_W);
  vec3 gw = depth * patGradToWorld(gradUV, N);
  return normalize(N + (gw - dot(gw, N) * N));
}
