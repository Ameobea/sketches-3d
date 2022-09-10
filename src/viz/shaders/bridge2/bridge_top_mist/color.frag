vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec4 outColor = vec4(0.8, 0.5, 0.6, 0.0);

  vec3 noisePos = pos;
  noisePos = quantize(noisePos, 0.02);
  // This is the speed at which the noise moves along the direction of the bridge
  noisePos.z += curTimeSeconds * -3.;
  // This is the speed at which the noise is pushed upwards, changing the pattern of the noise
  noisePos.y += curTimeSeconds * 1.;
  noisePos.x *= 6.;
  noisePos *= 3.;
  float noise = fbm_2_octaves(vec3(noisePos.x * 0.2, noisePos.y * 0.1, noisePos.z * 0.2));
  noise = pow(max(noise - 0.2, 0.), 0.62);
  // noise -= 0.3;
  noise = quantize(noise, 0.07);

  // Start de-activating the noise as we approach the platform
  float noiseFade = smoothstep(92., 108., pos.x);
  noise *= (1. - noiseFade);

  outColor.a = clamp(noise, 0.0, 1.0);

  return outColor;
}
