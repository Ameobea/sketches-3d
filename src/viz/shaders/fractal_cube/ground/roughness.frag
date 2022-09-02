float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec3 diffuse = ctx.diffuseColor.xyz;
  if (diffuse.x == 0.21) {
    return 1.;
  }

  float lightness = length(diffuse) / 3.;
  float roughness = 1. - pow(lightness * 1.04, 1.3);
  return roughness;
}
