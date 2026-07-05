// Wet glazed tiles, matte grout: roughness rises from the glossy face to the porous
// grout line. Specular is far more alias-sensitive than albedo (0.03→0.8 on a
// clearcoated face), so roughness filters against a doubled footprint and settles
// on the area-mean mix earlier than color does.
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  vec2 bd = ptBoundaryDist(ptProjectUV(pos, vWorldNormal), cl);
  return mix(PT_TILE_ROUGH, PT_GROUT_ROUGH, ptValleyVis(bd, 2. * ptAA()));
}
