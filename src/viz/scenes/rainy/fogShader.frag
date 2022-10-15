float linearize_depth(float d, float zNear, float zFar) {
  return zNear * zFar / (zFar + d * (zNear - zFar));
}

const vec4 FOG_COLOR = vec4(0.12, 0.12, 0.16, 1.);

void mainImage(const in vec4 inputColor, const in vec2 uv, const in float depth, out vec4 outputColor) {
  if (depth == 1.) {
    outputColor = vec4(FOG_COLOR.rgb, 0.5);
    return;
  }

  float linearDepth = linearize_depth(depth, cameraNear, cameraFar);
  float fogFactor = 1.0 - exp(-linearDepth * 0.08);
  // outputColor = mix(inputColor, FOG_COLOR, fogFactor * 0.9);
  outputColor = vec4(FOG_COLOR.rgb, fogFactor * 0.9);
  // outputColor = vec4(fogFactor, fogFactor, fogFactor, 1.);
}
