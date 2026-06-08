// Procedural light attenuation: fake cast shadow (→ direct) + fake AO
// (→ indirect), as (directMul, indirectMul). Applied after lighting, so it dims
// real light incl. specular — unlike the color shader, which only reaches albedo.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 sgns;
  vec3 d = triEdgeDist3(triProjectUV(pos, vWorldNormal), sgns);
  float ed = min(d.x, min(d.y, d.z));
  float aa = max(ctx.unitsPerPx, 1e-4);

  float cd = smoothstep(TRI_BORDER_END, TRI_WALL_END, ed);              // 0 top → 1 floor
  float pitMask = smoothstep(TRI_BORDER_END - aa, TRI_BORDER_END + aa, ed);

  // AO → indirect: depth darkening + contact darkening at the creases. The
  // smooth-min distance to the three rim creases dips at corners (two creases
  // near), so corners darken extra and the un-AO'd core rounds off.
  float depthAO = mix(1., TRI_AO_DEPTH, cd);
  float aoDist = triSmin(triSmin(abs(d.x - TRI_WALL_END), abs(d.y - TRI_WALL_END), TRI_AO_CORNER),
                         abs(d.z - TRI_WALL_END), TRI_AO_CORNER);
  float creaseAO = mix(TRI_AO_WALL, 1., smoothstep(0., TRI_AO_WALL_RANGE, aoDist));
  creaseAO = mix(1., creaseAO, pitMask);
  float indirectMul = depthAO * creaseAO;

  // Cast shadow → direct.
  float shadow = triPitShadowFromEdges(d, sgns, cd, normalize(vWorldNormal)) * pitMask;
  float directMul = mix(1., TRI_SHADOW_DARKEN, shadow);

  return vec2(directMul, indirectMul);
}
