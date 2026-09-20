vec4 sprite(vec2 uv, float seed, float age) {
  float r = length(uv - 0.5) * 2.0;
  float a = pow(max(1.0 - r, 0.0), 2.5);
  return vec4(1.0, 1.0, 1.0, a);
}
