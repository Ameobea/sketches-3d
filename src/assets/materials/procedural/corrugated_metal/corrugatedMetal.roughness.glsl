// Satin paint sheen on the rib crests, duller groove floors, worn-sheen streak
// variation; doubled footprint for specular AA.
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 p = patProjectUV(pos, vWorldNormal);
  vec2 aa = 2. * patAA();
  float r = mix(CM_ROUGH_TOP, CM_ROUGH_DEEP, cmCov(p.x, aa.x));
  r += 2. * CM_ROUGH_STREAK * cmStreakSignal(p, aa);
  return clamp(r, 0.05, 1.);
}
