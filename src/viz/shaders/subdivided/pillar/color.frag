vec3 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 newColor = baseColor * 1.4;
  // vec3 newColor = quantize(baseColor, 0.0);

  return newColor;
}
