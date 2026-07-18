// Albedo: warm cream tile face, muted grout in the valley (the rounded-rim shading
// comes from the relief normal, not albedo). ptValleyVis converges to the grout's
// duty-cycle mean at distance — the analytic mip — instead of dissolving to pure tile.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  vec2 bd = ptBoundaryDist(patProjectUV(pos, vWorldNormal), cl);
  vec3 col = mix(PT_TILE_COLOR, PT_GROUT_COLOR, ptValleyVis(bd, patAA()));
  return vec4(col, 1.);
}
