float getCustomMetalness(vec3 pos, vec3 normal, float baseMetalness, float curTimeSeconds, SceneCtx ctx) {
  float shinyness = pow(ctx.diffuseColor.r * 1.5, 7.) * 3.2;
  shinyness = clamp(shinyness, 0.2, 0.8);
  return shinyness;
}
