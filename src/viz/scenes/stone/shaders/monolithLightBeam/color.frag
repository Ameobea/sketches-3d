vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float alpha = 1. - smoothstep(1400., 3000., pos.y);
  return vec4(baseColor, alpha);
}
