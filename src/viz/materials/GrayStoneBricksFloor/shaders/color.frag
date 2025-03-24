const vec3[5] PLATFORM_COLOR_RAMP = vec3[5](vec3(0.0712, 0.091, 0.0904), vec3(0.0912, 0.131, 0.1304), vec3(0.22, 0.21, 0.27), vec3(0.52, 0.54, 0.73), vec3(0.22, 0.24, 0.23));

vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float brightness = fract(baseColor.r * 1.5);
  float rampIndex = brightness * float(5 - 1);
  int low = int(floor(rampIndex));
  int high = int(ceil(rampIndex));
  float t = fract(rampIndex);
  vec3 rampColor = mix(PLATFORM_COLOR_RAMP[low], PLATFORM_COLOR_RAMP[high], t);
  vec3 outColor = mix(rampColor, baseColor, 0.2);
  return vec4(outColor, 1.);
}
