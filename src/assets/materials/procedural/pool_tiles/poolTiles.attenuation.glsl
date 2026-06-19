// Recessed grout valley: spec-killed + AO. The glossy tile faces and rims keep
// full light, which is what sells the wet glaze.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  float b = ptBoundaryDist(ptProjectUV(pos, vWorldNormal), cl);
  if (b >= PT_EDGE_W) {
    return vec2(1.);
  }
  float v = ptValleyVis(b, max(ctx.aaFootprint, 1e-4));
  return vec2(mix(1., PT_DIRECT_GROUT, v), mix(1., PT_AO_GROUT, v));
}
