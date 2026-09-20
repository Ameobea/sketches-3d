#ifndef SP_SIZE
#define SP_SIZE 0.1
#endif
#ifndef SP_SPEED
#define SP_SPEED 0.5
#endif

float spHash(vec3 p) {
  p = fract(p * 0.3183099 + 0.1);
  p *= 17.0;
  return fract(p.x * p.y * p.z * (p.x + p.y + p.z));
}

float spNoise(vec3 x) {
  vec3 i = floor(x);
  vec3 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(
    mix(mix(spHash(i), spHash(i + vec3(1, 0, 0)), f.x), mix(spHash(i + vec3(0, 1, 0)), spHash(i + vec3(1, 1, 0)), f.x), f.y),
    mix(mix(spHash(i + vec3(0, 0, 1)), spHash(i + vec3(1, 0, 1)), f.x), mix(spHash(i + vec3(0, 1, 1)), spHash(i + vec3(1, 1, 1)), f.x), f.y),
    f.z
  );
}

Particle motion(float seed, float age, vec3 origin, vec4 params) {
  Particle p;
  float sz = SP_SIZE * mix(0.4, 2.2, params.x * params.x * params.x);
  vec3 q = origin * 0.045;
  vec3 flow = (vec3(spNoise(q), spNoise(q + 17.3), spNoise(q + 41.9)) - 0.5) * 2.0;
  vec3 wobble = vec3(
    sin(age * 0.31 + params.y * 6.2832),
    cos(age * 0.23 + params.z * 6.2832),
    sin(age * 0.27 + params.x * 6.2832)
  );
  p.pos = origin + (flow * SP_SPEED + wind) * age + wobble * sz * 6.0 - vec3(0.0, 0.12 * age * (0.5 + params.w), 0.0);
  p.size = sz;
  p.rot = params.y * 6.2832 + age * (params.z - 0.5) * 0.6;
  float tone = mix(0.08, 1.0, params.z * params.z);
  p.color = vec4(sporeColor * tone, mix(0.2, 0.6, params.w));
  return p;
}
