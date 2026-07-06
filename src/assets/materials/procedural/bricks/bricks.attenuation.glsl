// Recessed mortar joints: spec-killed + AO; block faces keep full light.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 brickId, cl;
  vec2 bd = brCellField(brProjectUV(pos, vWorldNormal), brickId, cl);
  float v = brJointVis(bd, brAA());
  return vec2(mix(1., BR_DIRECT_JOINT, v), mix(1., BR_AO_JOINT, v));
}
