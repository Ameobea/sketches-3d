float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(ctx.diffuseColor.r * 1.5, 7.);
  shinyness = clamp(shinyness, 0.0, 1.0);
  return 1. - shinyness;
}
