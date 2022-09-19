vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  // define 1D color LUT with 3 values
  vec3 colorLUT[3];
  colorLUT[0] = vec3(0.03, 0.03, 0.03);
  colorLUT[1] = vec3(0.14, 0.12, 0.1);
  colorLUT[2] = vec3(255./255., 211./255., 66./255.);

  float maxIx = float(colorLUT.length());
  float index = clamp(pow(baseColor.r,1.1) * maxIx, 0.0, maxIx + 0.9999);
  // linear interpolation between 2 values
  vec3 color = mix(colorLUT[int(index)], colorLUT[int(index) + 1], fract(index));

  return vec4(color, 1.0);
  // return vec4(1. - color.r, 1. - color.r, 1. - color.r, 1.0);
}
