float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 oPos = pos.xz * 0.5;

  // [-1, 1]
  float noise0 = fbm(oPos * 2.);
  // [0, 1]
  noise0 = noise0 * 0.5 + 0.5;
  noise0 = pow(noise0, 3.);
  noise0 = quantize(noise0, 0.1);
  noise0 = noise0 * 2.;
  noise0 = noise0 + 0.2;
  noise0 = clamp(noise0, 0., 1.);

  return noise0;
}
