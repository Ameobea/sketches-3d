// Wood albedo darkening into the recesses (finish/dust pooling), overlaid with
// fbm grain streaked along the trim direction and dissolved with footprint.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 p = patProjectUV(pos, vWorldNormal);
  vec2 aa = patAA();
  vec3 col = mix(MD_COLOR, MD_COLOR_DEEP, mdCov(p, aa));

// The fbm domain isn't wrap-periodic, so a planar sample would seam at the v-wrap
// (pattern v jumps UV_SCALE.y → 0); embed v on a cylinder to make it periodic.
#if PAT_UV_MODE == 1
  float ang = 6.2832 * vUv.y;
  float grain = fbm(vec3(p.x * MD_GRAIN_SCALE.x, cos(ang) * MD_GRAIN_CYL_R, sin(ang) * MD_GRAIN_CYL_R)) - 0.47;
#else
  float grain = fbm(p * MD_GRAIN_SCALE) - 0.47;
#endif
  float gaa = max(aa.x * MD_GRAIN_SCALE.x, aa.y * MD_GRAIN_SCALE.y);
  col *= 1. + 2. * MD_GRAIN_AMP * grain * (1. - fadeToMeanFactor(gaa, 1.5));
  return vec4(max(col, 0.), 1.);
}
