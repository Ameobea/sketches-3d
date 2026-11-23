vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float alpha = 1. - smoothstep(250., 300., pos.y);
  alpha = clamp(sin(curTimeSeconds * 2.) * 0.5 + 0.7, 0., 1.) * alpha;
  // analytic soft silhouette: normalized distance of the view ray from the beam axis
  vec2 n = normalize(normal.xz);
  vec2 v = normalize(ctx.cameraPosition.xz - pos.xz);
  alpha *= 1. - smoothstep(0.3, 1., abs(n.x * v.y - n.y * v.x));
  return vec4(baseColor, alpha);
}
