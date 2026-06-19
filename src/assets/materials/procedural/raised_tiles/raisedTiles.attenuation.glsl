// Fake contact-AO (→ indirect) + fake self-shadow (→ direct), no second march.
// For each edge whose neighbour is taller, this (recessed) tile gets: a short
// symmetric contact-darkening crease (AO) and, on the side the fixed face-space
// key (RT_LIGHT_UV, "from above") is blocked, a longer directional cast shadow.
// The light is a constant in projected face space — scene-independent and
// consistent on every face, which is what makes the shadow read cleanly.

// Per-axis edge. `ao` out = contact-AO coverage; return = cast-shadow coverage.
float rtEdge(vec2 cl, vec2 axis, vec2 cellId, float cThis, out float ao) {
  float along = dot(cl, axis);
  vec2 nrm = axis * sign(along);
  float dist = 0.5 * RT_CELL - abs(along);
  float dh = cThis - rtTileCarve(cellId + nrm); // >0 ⇒ neighbour taller (we're recessed)
  ao = 0.0;
  if (dh <= 0.0) {
    return 0.0;
  }
  float dhn = clamp(dh / RT_TILE_SPAN, 0.0, 1.0);
  ao = dhn * (1.0 - smoothstep(0.0, RT_AO_REACH, dist));
  float facing = dot(RT_LIGHT_UV, nrm);
  if (facing <= 0.0) {
    return 0.0;
  }
  float reach = dh * RT_SHADOW_REACH * facing;
  return 1.0 - smoothstep(0.0, max(reach, 1e-4), dist);
}

vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cellId, cl, edgeDir;
  rtCellField(rtProjectUV(pos, vWorldNormal), cellId, cl, edgeDir);
  float aa = max(ctx.aaFootprint, 1e-4);
  float cThis = rtTileCarve(cellId);
  // Off past the POM cutoff (relief is flat there); also dissolve with footprint.
  float fade = max(fadeToMeanFactor(aa, RT_FADE_PERIOD), rtPomFade(ctx.distanceToCamera));

  float aoX, aoY;
  float sh = max(rtEdge(cl, vec2(1., 0.), cellId, cThis, aoX),
                 rtEdge(cl, vec2(0., 1.), cellId, cThis, aoY));
  float ao = max(aoX, aoY);

  float indirect = mix(mix(1.0, RT_AO_RECESS, ao), 1.0, fade);
  float direct = mix(1.0, RT_SHADOW_DARKEN, mix(sh, 0.0, fade));
  return vec2(direct, indirect);
}
