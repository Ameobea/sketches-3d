vec3 getCustomEmissive(vec3 pos, vec3 emissive, float curTimeSeconds, SceneCtx ctx) {
  vec3 diffuse = ctx.diffuseColor.xyz;

  float emissiveActivation = smoothstep(0., 1., diffuse.x);
  emissiveActivation = 1. - emissiveActivation;
  return mix(vec3(0.), vec3(1., 0., 0.), emissiveActivation * 0.08);
}
