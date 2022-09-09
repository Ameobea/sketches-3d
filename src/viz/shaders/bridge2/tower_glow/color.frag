vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float fadeOutStart = 32.4;
  float fadeOutEnd = 38.;
  // float fadeExponent = 2. + sin(curTimeSeconds);
  float fadeExponent = 1.4;

  float fadeOutActivation = 1. - smoothstep(fadeOutStart, fadeOutEnd, abs(pos.z));
  float fadeUpActivation = 1. - smoothstep(280., 330., pos.y);
  float alpha = pow(fadeOutActivation, fadeExponent) * fadeUpActivation;

  // if (abs(pos.z) > fadeOutStart) {
  //   return vec4(1., 0., 0., alpha);
  // } else if (abs(pos.z) > fadeOutEnd) {
  //   return vec4(0., 1., 0., alpha);
  // }

  return vec4(0., 0., 0., alpha);
}
