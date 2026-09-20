vec4 sprite(vec2 uv, float seed, float age) {
  vec2 d = uv - 0.5;
  float ang = atan(d.y, d.x);
  float wob = 1.0 + 0.18 * sin(ang * 3.0 + seed * 40.0) + 0.1 * sin(ang * 5.0 + seed * 71.0);
  float a = 1.0 - smoothstep(0.45, 1.0, length(d) * 2.0 / wob);
  return vec4(1.0, 1.0, 1.0, a * a * 0.2);
}
