// Procedural light attenuation: fake cast shadow (→ direct) + fake AO (→ indirect), as
// (directMul, indirectMul). Applied after lighting, so it dims real light incl. specular.
// Reads the shared at-hit frame (TriHit).
vec2 gridAttenuation(TriHit h, SceneCtx ctx) {
  vec3 d = h.d;
  vec3 sgns = h.sgns;
  float ed = min(d.x, min(d.y, d.z));
  float aa = max(ctx.aaFootprint, 1e-4);

  float cd = smoothstep(TRI_BORDER_END, TRI_WALL_END, ed); // 0 top → 1 floor
  float pitMask = smoothstep(TRI_BORDER_END - aa, TRI_BORDER_END + aa, ed);

  // AO → indirect: depth darkening + corner-biased contact darkening at the rim creases.
  float depthAO = mix(1., TRI_AO_DEPTH, cd);
  float aoDist = triSmin(triSmin(abs(d.x - TRI_WALL_END), abs(d.y - TRI_WALL_END), TRI_AO_CORNER),
                         abs(d.z - TRI_WALL_END), TRI_AO_CORNER);
  float creaseAO = mix(TRI_AO_WALL, 1., smoothstep(0., TRI_AO_WALL_RANGE, aoDist));
  creaseAO = mix(1., creaseAO, pitMask);
  float indirectMul = depthAO * creaseAO;

  // Cast shadow → direct. vWorldNormal is used only scale-invariantly (dot-sign + abs axis pick).
  float shadow = triPitShadowFromEdges(d, sgns, cd, vWorldNormal) * pitMask;
  float directMul = mix(1., TRI_SHADOW_DARKEN, shadow);

  return vec2(directMul, indirectMul * mix(directMul, 1., 0.5));
}
