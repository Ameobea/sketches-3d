// Padded-carve-driven dimming: AO toward the gap floor plus a mild direct dim —
// the gaps still catch sun, and the bevels keep their specular.
vec2 gridAttenuation(SeHit h, SceneCtx ctx) {
  float carve = seVisCarveHit(h);
  return vec2(mix(1., SE_DIRECT_FLOOR, carve), mix(1., SE_AO_FLOOR, carve));
}
