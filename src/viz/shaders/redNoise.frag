vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 outColor = quantize(baseColor, 0.11);

  float pulse = sin(curTimeSeconds * 2.5) * 0.5 + 0.5;
  vec3 pulseColor = vec3(0.538, 0.538, 0.538) * 0.1;
  outColor = mix(baseColor, pulseColor, pulse * 0.4 + 0.15);

  float flickerVal = noise(curTimeSeconds * 1.5);
  float flickerActivation = smoothstep(0.4, 1.0, flickerVal * 2. + 0.2);
  if (flickerActivation < 0.1) {
    return vec4(outColor, 1.);
  }

  vec3 noisePos = pos.xyz * 0.06;
  // noisePos += vec2(curTimeSeconds * -1.2, 0.);
  noisePos.x *= 1.5;
  float noiseFreq = 1.4;
  float noiseLacunarity = 2.4;
  float noisePersistence = 5.3;
  float noiseAttenuation = 4.;
  float noiseVal = ridged_multifractal_noise(
    noisePos,
    noiseFreq,
    noiseLacunarity,
    noisePersistence,
    noiseAttenuation
  );
  noiseVal = sin(noiseVal * 5. + sin(curTimeSeconds * 0.04) * 40. + 12.);
  // [-1, 1] -> [0, 1]
  noiseVal = noiseVal * 0.5 + 0.5;

  noiseVal = pow(noiseVal, 1.4);

  // array of colors
  vec3 colors[5] = vec3[5](
    vec3(0, 0, 4),
    vec3(46, 8, 1),
    vec3(105, 7, 2),
    // vec3(31, 17, 87),
    vec3(73, 2, 2),
    vec3(43, 2, 2)
    // vec3(65, 32, 129),
    // vec3(18, 57, 73),
    // vec3(33, 49, 136)
  );
  // Linear interpolation between colors for noise [-1, 1]
  float colorIndex = noiseVal * float(colors.length() - 1);
  float colorIndexFloor = floor(colorIndex);
  float colorIndexFrac = colorIndex - colorIndexFloor;
  vec3 noiseColor1 = colors[int(colorIndexFloor)];
  vec3 noiseColor2 = colors[int(colorIndexFloor) + 1];
  vec3 noiseColor = mix(noiseColor1, noiseColor2, colorIndexFrac) / 255.;
  noiseColor = quantize(noiseColor, 0.11);

  // vec3 flickerColor = vec3(0.45, 0.008, 0.008);
  vec3 flickerColor = noiseColor;
  outColor = mix(outColor, flickerColor, flickerActivation);
  // return flickerColor;

  // outColor += quantize(noiseColor, 0.15);
  outColor.x = clamp(outColor.x, 0., 1.);
  outColor.y = clamp(outColor.y, 0., 1.);
  outColor.z = clamp(outColor.z, 0., 1.);

  // outColor = quantize(outColor, 0.02);
  return vec4(outColor, 1.);
}
