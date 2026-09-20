// x/y noise frequency in sprite space; unequal values pull the puff into strands
#ifndef WISP_FREQ
#define WISP_FREQ vec2(1.1, 2.6)
#endif
// silhouette squash across the strand direction; 1 = round
#ifndef WISP_ASPECT
#define WISP_ASPECT 2.2
#endif
// 0 = smooth puff, 1 = heavily eroded filaments
#ifndef WISP_EROSION
#define WISP_EROSION 0.8
#endif

float wHash(vec2 p) {
  p = fract(p * vec2(0.1031, 0.1030));
  p += dot(p, p.yx + 33.33);
  return fract((p.x + p.y) * p.x);
}

float wNoise(vec2 x) {
  vec2 i = floor(x);
  vec2 f = fract(x);
  f = f * f * (3.0 - 2.0 * f);
  return mix(mix(wHash(i), wHash(i + vec2(1, 0)), f.x), mix(wHash(i + vec2(0, 1)), wHash(i + vec2(1, 1)), f.x), f.y);
}

float wFbm(vec2 x) {
  const mat2 rot = mat2(1.6, 1.2, -1.2, 1.6);
  float v = 0.5 * wNoise(x);
  x = rot * x;
  v += 0.25 * wNoise(x);
  x = rot * x;
  return (v + 0.125 * wNoise(x)) / 0.875;
}

vec4 sprite(vec2 uv, float seed, float age) {
  vec2 d = (uv - 0.5) * 2.0;
  float window = 1.0 - smoothstep(0.55, 1.0, dot(d, d));
  if (window <= 0.0) return vec4(0.0);
  vec2 q = d * WISP_FREQ + seed * 91.7 + vec2(age * 0.02, 0.0);
  vec2 warp = vec2(wNoise(q * 0.7 + 7.1), wNoise(q * 0.7 - 3.3)) - 0.5;
  vec2 e = (d + warp * 0.8) * vec2(1.0, WISP_ASPECT);
  float falloff = max(1.0 - dot(e, e), 0.0);
  falloff *= falloff * window;
  float body = smoothstep(0.35, 0.8, wFbm(q + warp * 1.5) + falloff * 0.3);
  return vec4(1.0, 1.0, 1.0, falloff * mix(1.0, body, WISP_EROSION));
}
