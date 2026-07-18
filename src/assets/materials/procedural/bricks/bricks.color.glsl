// Albedo layers, base → surface: per-brick tinted block / mortar joint mix, then
// block-scale stain + fine aggregate grain (grain dissolved with footprint), then
// soot patches over everything — grime sits on blocks and joints alike. Per-brick
// jitter fades with footprint so subpixel bricks don't shimmer.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = patProjectUV(pos, vWorldNormal);
  vec2 brickId, cl;
  vec2 bd = brCellField(uv, brickId, cl);
  vec2 aa = patAA();
  float aaS = max(aa.x, aa.y);

  float keep = 1. - fadeToMeanFactor(aaS, min(BR_CELL.x, BR_CELL.y));
  vec3 block = BR_BLOCK_COLOR;
  block *= 1. + 2. * BR_TINT_AMP * keep * (hash(brickId + 7.31) - 0.5);
  block *= 1. - 0.12 * keep * smoothstep(0.7, 1., hash(brickId + 3.7));

  vec3 col = mix(block, BR_MORTAR_COLOR, brJointVis(bd, aa));

  float mottle = fbm(uv * 0.7 + 13.) - 0.5;
  // Aggregate speckle: centered fbm grit plus thresholded darker pits, both
  // dissolved with footprint so the grit never shimmers at distance.
  float g = fbm(uv * BR_GRAIN_SCALE);
  float grain = 2. * (g - 0.5) - 1.4 * smoothstep(0.58, 0.78, g);
  grain *= 1. - fadeToMeanFactor(aaS, 1.7 / BR_GRAIN_SCALE);
  col *= 1. + 2.2 * BR_MOTTLE_AMP * mottle + BR_GRAIN_AMP * grain;

  col = mix(col, BR_SOOT_COLOR, brSoot(uv));
  return vec4(max(col, 0.), 1.);
}
