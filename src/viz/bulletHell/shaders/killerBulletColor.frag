vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  int stepIx = int(curTimeSeconds * 4.);
  return stepIx % 2 == 0 ? vec4(1., 0.1, 0.1, 1) : vec4(0.5, 0.03, 0.03, 1.);
}
