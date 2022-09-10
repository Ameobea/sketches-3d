float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float roughness = baseRoughness;
  roughness = roughness * 0.4;
  return 1. - roughness;
}
