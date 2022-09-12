vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float distanceToMonolithPillar = 400. - ctx.cameraPosition.x;
  float activation = 1. - smoothstep(10., 150., distanceToMonolithPillar);

  vec3 outColor = clamp(baseColor * (sin(curTimeSeconds) * 0.5 + 0.5 + 0.3), 0.0, 1.0);
  outColor = mix(baseColor, outColor, activation);
  return vec4(outColor, 1.0);
}
