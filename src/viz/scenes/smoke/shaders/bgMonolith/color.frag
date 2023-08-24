vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float alpha = 0.;
  if (pos.y < 0.) {
    alpha = smoothstep(-400., -150., pos.y);
  } else {
    alpha = smoothstep(140., 400., pos.y);
    alpha = 1. - alpha;
  }

  return vec4(baseColor, alpha);
}
