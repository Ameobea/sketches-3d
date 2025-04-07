float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float outRoughness = 1. - ctx.diffuseColor.r;
  outRoughness *= 0.8;

  return outRoughness;
}
