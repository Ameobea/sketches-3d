// Procedural light attenuation from the shared at-hit frame: fake AO (→ indirect) + fake crevice
// cast shadow (→ direct), as (directMul, indirectMul). cf. triangleGrid.attenuation.glsl.
vec2 gridAttenuation(ChHit h, SceneCtx ctx) {
  float aw = h.aw;
  float aa = max(ctx.aaFootprint, 1e-4) * CH_G;
  float cd = chCarveVS(aw).x / CH_CARVE;                                 // 0 ridge top → 1 floor
  float crevice = 1. - smoothstep(CH_BEVEL_END - aa, CH_BEVEL_END + aa, aw);

  float depthAO = mix(1., CH_AO_DEPTH, cd);
  float creaseAO = mix(CH_AO_WALL, 1., smoothstep(0., CH_AO_WALL_RANGE, abs(aw - CH_FLOOR_HW)));
  float indirectMul = depthAO * mix(1., creaseAO, crevice);

  float shadow = chCreviceShadow(h.off * aw, h.su, cd, vWorldNormal) * crevice;
  float directMul = mix(1., CH_SHADOW_DARKEN, shadow);
  return vec2(directMul, indirectMul);
}
