float getCustomRoughness(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec3 outNormal = vec3(0., 0., 1.);

  vec3 oPos = pos;
  // oPos = quantize(pos, 0.02);
  oPos.x += curTimeSeconds * 0.08;
  oPos.x *= 2.;
  oPos.y += curTimeSeconds * 0.08;
  oPos.z += curTimeSeconds * 0.08;

  // [-1, 1]
  float noise0 = fbm(oPos * 2.);
  // [0, 1]
  noise0 = noise0 * 0.5 + 0.5;
  noise0 = pow(noise0, 2.);
  noise0 = quantize(noise0, 0.1);

  return noise0;
}
