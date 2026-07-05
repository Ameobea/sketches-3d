// Recessed grout valley: spec-killed + AO. The glossy tile faces and rims keep
// full light, which is what sells the wet glaze.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  vec2 bd = ptBoundaryDist(ptProjectUV(pos, vWorldNormal), cl);
  float v = ptValleyVis(bd, ptAA());
  return vec2(mix(1., PT_DIRECT_GROUT, v), mix(1., PT_AO_GROUT, v));
}
