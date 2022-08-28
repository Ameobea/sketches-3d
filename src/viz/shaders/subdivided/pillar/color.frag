vec3 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 newColor = baseColor;
  vec3 hsv = rgb2hsv(newColor);
  hsv.x = sin(curTimeSeconds * 0.5) * 0.5 + 0.5;
  hsv.y += 0.2;
  newColor = hsv2rgb(hsv);

  newColor = quantize(newColor, 0.25);

  return newColor;
}
