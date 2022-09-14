vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  // Fade to black as y goes lower from 0
  float blackActivation = smoothstep(-12., 10., pos.y);
  vec3 fadedColor = mix(vec3(0.0), baseColor, blackActivation);
  return vec4(fadedColor, 1.0);
}
