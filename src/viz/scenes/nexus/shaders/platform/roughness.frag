float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(ctx.diffuseColor.b * 24.5, 2.5) * 0.2;
  shinyness = clamp(shinyness, 0.0, 0.6);
  return 1. - shinyness;
}
