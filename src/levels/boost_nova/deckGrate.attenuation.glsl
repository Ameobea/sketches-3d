// Extra contact darkening in the carved grooves, layered on top of the POM relief shading.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float aa = max(ctx.aaFootprint, 1e-4);
  float gap = dgGap(pomMeshUv(pos), aa);
  return vec2(mix(1., DG_DIRECT_GAP, gap), mix(1., DG_AO_GAP, gap));
}
