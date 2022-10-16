float linearize_depth(float d, float zNear, float zFar) {
  return zNear * zFar / (zFar + d * (zNear - zFar));
}

const vec4 FOG_COLOR = vec4(0.15, 0.15, 0.22, 1.);

const vec3[2] FOG_COLOR_RAMP = vec3[2](
  vec3(0.15, 0.15, 0.22),
  vec3(0.1, 0.1, 0.13)
);

void mainImage(const in vec4 inputColor, const in vec2 uv, const in float depth, out vec4 outputColor) {
  if (depth == 1.) {
    outputColor = inputColor;
    return;
  }

  float linearDepth = linearize_depth(depth, cameraNear, cameraFar);
  float fogFactor = 1.0 - exp(-linearDepth * 0.08);
  vec3 fogColor = mix(FOG_COLOR_RAMP[0], FOG_COLOR_RAMP[1], fogFactor);
  outputColor = mix(inputColor, vec4(fogColor, 1.), fogFactor * 0.9);
  // outputColor = vec4(fogColor, fogFactor * 0.9);
  // outputColor = vec4(fogFactor, fogFactor, fogFactor, 1.);
}
