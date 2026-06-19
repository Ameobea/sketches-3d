// Wet glazed tiles, matte grout: roughness rises from the glossy face to the
// porous grout line in the valley.
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  float b = ptBoundaryDist(ptProjectUV(pos, vWorldNormal), cl);
  float v = b < PT_EDGE_W ? ptValleyVis(b, max(ctx.aaFootprint, 1e-4)) : 0.;
  return mix(PT_TILE_ROUGH, PT_GROUT_ROUGH, v);
}
