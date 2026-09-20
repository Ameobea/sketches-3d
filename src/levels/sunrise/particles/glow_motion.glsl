Particle motion(float seed, float age, vec3 origin, vec4 params) {
  Particle p;
  float sz = GLOW_SIZE * mix(0.6, 1.4, params.x);
  vec3 wobble = vec3(
    sin(age * 0.19 + params.y * 6.2832),
    sin(age * 0.11 + params.z * 6.2832) * 0.5,
    cos(age * 0.17 + params.x * 6.2832)
  );
  p.pos = origin + wind * age + wobble * sz * 5.0 + vec3(0.0, 0.08 * age, 0.0);
  p.size = sz;
  p.rot = 0.0;
  float pulse = 0.55 + 0.45 * sin(age * (0.4 + params.w) + params.y * 6.2832);
  p.color = vec4(glowColor * pulse, 0.35);
  return p;
}
