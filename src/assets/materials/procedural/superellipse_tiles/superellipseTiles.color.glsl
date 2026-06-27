// Albedo follows the padded visual carve: warm light tops, dark gaps, the
// bevel blending between. All gradients are C1-smooth so no per-edge AA is
// needed beyond the profile itself.
vec4 gridColor(SeHit h, vec3 baseColor, SceneCtx ctx) {
  float carve = seVisCarveHit(h);
  return vec4(mix(SE_TOP_COLOR, SE_FLOOR_COLOR, carve), 1.);
}
