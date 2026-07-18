// Two-tone albedo: flat top vs darker recesses (pits + grooves), dissolved to the
// duty mean at distance. Wall shading comes from the relief normal, not albedo.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 p = ssCellLocal(patProjectUV(pos, vWorldNormal));
  return vec4(mix(SS_TOP_COLOR, SS_PIT_COLOR, ssCov(p, patAA())), 1.);
}
