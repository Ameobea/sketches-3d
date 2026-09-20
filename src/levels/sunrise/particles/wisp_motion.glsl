#ifndef WISP_SIZE
#define WISP_SIZE 7.0
#endif
#ifndef WISP_ALPHA
#define WISP_ALPHA 0.1
#endif
// spread of strand directions around screen-horizontal, radians
#ifndef WISP_TILT
#define WISP_TILT 1.2
#endif
#ifndef WISP_MEANDER
#define WISP_MEANDER 3.0
#endif

Particle motion(float seed, float age, vec3 origin, vec4 params) {
  Particle p;
  float t = age / pLifetime;
  vec3 meander = vec3(
    sin(age * 0.13 + params.y * 6.2832) + 0.5 * sin(age * 0.29 + params.z * 6.2832),
    0.4 * sin(age * 0.17 + params.w * 6.2832),
    cos(age * 0.11 + params.x * 6.2832) + 0.5 * cos(age * 0.23 + params.y * 6.2832)
  );
  p.pos = origin + wind * age + meander * WISP_MEANDER;
  p.size = WISP_SIZE * mix(0.6, 1.6, params.x) * mix(0.55, 1.0, t);
  p.rot = (params.z - 0.5) * WISP_TILT + age * (params.w - 0.5) * 0.06;
  float fade = smoothstep(0.0, 0.25, t) * (1.0 - smoothstep(0.6, 1.0, t));
  p.color = vec4(wispColor * mix(0.7, 1.15, params.y), WISP_ALPHA * fade * mix(0.5, 1.0, params.w));
  return p;
}
