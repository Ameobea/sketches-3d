// Groove AO: recesses lose indirect (and a little direct) light — most of what
// sells the corrugation at mid distance. Too shallow for a cast shadow to read.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float cov = cmCov(patProjectUV(pos, vWorldNormal).x, patAA().x);
  return vec2(mix(1., CM_DIRECT_DEEP, cov), mix(1., CM_AO_DEEP, cov));
}
