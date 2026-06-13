// Albedo follows the padded visual carve: warm light tops, dark gaps, the
// bevel blending between. All gradients are C1-smooth so no per-edge AA is
// needed beyond the profile itself.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl = seCellLocal(seProjectUV(pos, vWorldNormal));
  float carve = seVisCarve(cl, seRadius(cl));
  return vec4(mix(SE_TOP_COLOR, SE_FLOOR_COLOR, carve), 1.);
}
