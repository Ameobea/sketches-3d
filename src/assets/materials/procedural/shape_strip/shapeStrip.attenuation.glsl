// Fake AO inside the recesses; direct kept so wall highlights survive.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 p = ssCellLocal(patProjectUV(pos, vWorldNormal));
  return vec2(1., mix(1., SS_AO, ssCov(p, patAA())));
}
