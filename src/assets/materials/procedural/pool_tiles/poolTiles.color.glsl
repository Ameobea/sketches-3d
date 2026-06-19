// Albedo: warm cream tile face, muted grout line in the valley (the rounded-rim
// shading comes from the relief normal, not albedo). Footprint-AA'd via
// ptValleyVis, which dissolves the thin grout grid as it goes sub-pixel.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  float b = ptBoundaryDist(ptProjectUV(pos, vWorldNormal), cl);
  vec3 col = PT_TILE_COLOR;
  if (b < PT_EDGE_W) {
    col = mix(col, PT_GROUT_COLOR, ptValleyVis(b, max(ctx.aaFootprint, 1e-4)));
  }
  return vec4(col, 1.);
}
