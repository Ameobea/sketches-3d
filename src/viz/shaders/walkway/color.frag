vec3 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 outColor = baseColor;

  vec3 newColor = vec3(1., 0., 0.);
  // float g = fbm(pos + curTimeSeconds) * 0.5 + 0.5;
  // float b = fbm(pos + -curTimeSeconds) * 0.5 + 0.5;

  // float rainbowActivation = smoothstep(-1., 1., sin(curTimeSeconds * 0.7));
  // outColor.y = g * rainbowActivation, 0.2;
  // outColor.z = b * rainbowActivation, 0.2;

  float mixFactor = sin(curTimeSeconds * 1.1 + pos.x * 0.4);
  mixFactor = mixFactor * 0.5 + 0.5;
  outColor = mix(outColor, newColor, mixFactor);

  float xNoise = noise(pos.x * 4. + curTimeSeconds * 1.);
  if (distance(pos, ctx.cameraPosition) > 1000.) {
    xNoise += noise((pos.x + 0.1) * 4. + curTimeSeconds * 1.);
    xNoise *= 0.5;
  }
  if (xNoise > 0.8) {
    // outColor *= 0.1;
  }
  float yNoise = noise(pos.y * 4. + 11.23);
  if (yNoise > 0.8) {
    outColor *= 0.1;
  }
  float zNoise = noise(pos.z * 4. + curTimeSeconds * 1.);
  if (distance(pos, ctx.cameraPosition) > 1000.) {
    zNoise += noise((pos.z + 1.) * 4. + curTimeSeconds * 1.);
    zNoise *= 0.5;
  }
  vec3 zNoiseApplied = outColor * 0.1;
  outColor = mix(outColor, zNoiseApplied, smoothstep(0.6, 1., zNoise));

  return outColor;
}
