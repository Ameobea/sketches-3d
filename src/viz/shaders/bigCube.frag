vec3 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 outColor = baseColor;

  float flickerVal = noise(curTimeSeconds * 1.5);
  float flickerActivation = smoothstep(0.4, 1.0, flickerVal * 2. + 0.2);

  vec3 noisePos = pos * 8.1;
  noisePos.z *= 6.1;
  noisePos.z += noise(0.5 * noisePos.y) * 4.3;
  noisePos.y += curTimeSeconds * 135.5;

  float noiseVal = fbm(noisePos);
  noiseVal = pow(noiseVal, 1.5);
  noiseVal = quantize(noiseVal, 0.05);

  // We want to limit the areas where noise is drawn to vertical bands along the z axis
  float noiseActivation = noisePos.z * 0.05;
  noiseActivation += fbm(pos.y + 0.05 * noisePos.y + 0.08) * 20. * flickerActivation * ( 0.032 * -cos(curTimeSeconds * 85.5)) + 2.6 * curTimeSeconds;
  noiseActivation = noise(noiseActivation);
  noiseActivation = smoothstep(0.63, 1., noiseActivation);
  noiseVal = mix(0., noiseVal, noiseActivation);

  vec3 noiseColor = mix(vec3(noiseVal, noiseVal, noiseVal), vec3(noiseVal, 0., 0.), flickerActivation);

  outColor += noiseColor * (flickerActivation + 0.2);
  outColor.x = clamp(outColor.x, 0.0, 1.0);
  outColor.y = clamp(outColor.y, 0.0, 1.0);
  outColor.z = clamp(outColor.z, 0.0, 1.0);

  return outColor;
}
