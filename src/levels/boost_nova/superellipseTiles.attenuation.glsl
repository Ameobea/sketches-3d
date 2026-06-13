// Padded-carve-driven dimming: AO toward the gap floor plus a mild direct dim —
// the gaps still catch sun, and the bevels keep their specular.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl = seCellLocal(seProjectUV(pos, vWorldNormal));
  float carve = seVisCarve(cl, seRadius(cl));
  return vec2(mix(1., SE_DIRECT_FLOOR, carve), mix(1., SE_AO_FLOOR, carve));
}
