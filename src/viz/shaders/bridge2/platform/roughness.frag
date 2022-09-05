float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  baseRoughness = 1. - max(ctx.diffuseColor.x, max(ctx.diffuseColor.y, ctx.diffuseColor.z));
  float roughnessActivation = smoothstep(0.5, 1., baseRoughness);
  float roughness = mix(baseRoughness, 1., roughnessActivation) * 1.4;
  roughness = clamp(roughness, 0., 1.);

  // Fade to black as y goes from 0 to -20
  float blackActivation = smoothstep(-20., 0., pos.y);
  float fadedRoughness = mix(1., roughness, blackActivation);
  return fadedRoughness;
}
