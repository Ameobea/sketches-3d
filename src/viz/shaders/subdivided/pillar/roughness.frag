float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 adjVUv = ctx.vUv * 6. * PI;
  vec3 oPos = vec3(adjVUv.x, adjVUv.y, curTimeSeconds * 0.2);

  // [-1, 1]
  float noise0 = fbm(oPos * 2.);
  // [0, 1]
  noise0 = noise0 * 0.5 + 0.5;

  return noise0;
}
