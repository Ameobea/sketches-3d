// Finish sheen on the crests, duller recesses; doubled footprint for specular AA.
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  return mix(MD_ROUGH_TOP, MD_ROUGH_DEEP, mdCov(mdProjectUV(pos, vWorldNormal), 2. * mdAA()));
}
