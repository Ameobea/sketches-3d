vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float alpha = 1. - smoothstep(250., 300., pos.y);
  alpha = clamp(sin(curTimeSeconds * 2.) * 0.5 + 0.7, 0., 1.) * alpha;
  return vec4(baseColor, alpha);
}
