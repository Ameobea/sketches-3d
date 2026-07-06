// Recess AO: valleys and the inter-row gap lose indirect (and a little direct)
// light, which is most of what sells the carved read at mid distance.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float cov = mdCov(mdProjectUV(pos, vWorldNormal), mdAA());
  return vec2(mix(1., MD_DIRECT_DEEP, cov), mix(1., MD_AO_DEEP, cov));
}
