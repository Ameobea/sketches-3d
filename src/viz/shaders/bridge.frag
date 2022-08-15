vec3 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds) {
  // rotating color using sine based on position.x
  // float r = sin(pos.x * 0.1) * 0.5 + 0.5;
  // float g = sin(pos.x * 0.1 + 1.0) * 0.5 + 0.5;
  // float b = sin(pos.x * 0.1 + 2.0) * 0.5 + 0.5;
  // vec3 newColor = vec3(r, g, b);
  vec3 newColor = baseColor;

  // add noise to the color
  vec3 noisePos = pos;
  // This is the speed at which the noise moves along the direction of the bridge
  noisePos.x += curTimeSeconds * -15.;
  // This is the speed at which the noise is pushed upwards, changing the pattern of the noise
  noisePos.y += curTimeSeconds * 7.;
  noisePos.z *= 12.;
  float noise = fbm(noisePos * 0.1);
  noise = pow(max(noise - 0.2, 0.), 0.62);
  noise = quantize(noise, 0.1);

  float steepness = acos(normal.y) / PI;
  // As steepness increases, noise decreases
  float noiseMix = pow(1. - smoothstep(0.05, 0.5, steepness), 2.);
  noise *= noiseMix;

  // Darken steep parts
  newColor = mix(newColor, vec3(0.03), 1. - noiseMix);

  newColor -= noise * 0.4;
  newColor.x = max(newColor.x, 0.008);
  newColor.y = max(newColor.y, 0.008);
  newColor.z = max(newColor.z, 0.008);

  return newColor;
}
