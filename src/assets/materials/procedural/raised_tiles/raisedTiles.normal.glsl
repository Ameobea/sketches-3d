// Closed-form relief normal for the step wall. carve = mix(mid, cThis,
// smoothstep(0, WALL_W, b)) with mid = (cThis+cNb)/2, so ∂carve/∂b =
// ½(cThis−cNb)·ss'(b) and ∇_uv carve = -∂carve/∂b·edgeDir. Mirrors the height
// slot — keep in sync. Flat (→ N) on the plateau.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 cellId, cl, edgeDir;
  float b = rtCellField(rtProjectUV(pos, N), cellId, cl, edgeDir);
  if (b >= RT_WALL_W) {
    return N;
  }

  float cThis = rtTileCarve(cellId);
  float cNb = rtTileCarve(cellId + edgeDir);
  float u = b / RT_WALL_W;
  float dCarveDb = 0.5 * (cThis - cNb) * 6.0 * u * (1.0 - u) / RT_WALL_W;
  vec2 gradUV = -dCarveDb * edgeDir;
  gradUV *= reliefAAFade(aa, RT_WALL_W);

  vec3 gw = depth * rtUVtoWorld(gradUV, N);
  return normalize(N + (gw - dot(gw, N) * N));
}
