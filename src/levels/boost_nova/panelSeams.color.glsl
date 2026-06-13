// Near-monochrome albedo: dark plastic base, faintly tinted seam (the chamfer
// shading comes from the relief normal, not albedo). Oversampled
// (antialiasColorShader) so the gap line averages toward coverage at distance.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl;
  float b = psBoundaryDist(psProjectUV(pos, vWorldNormal), cl);
  vec3 col = PS_BASE_COLOR;
  if (b < PS_SEAM_W) {
    vec2 sv = psSeamVis(b, max(ctx.unitsPerPx, 1e-4));
    col = mix(col, PS_DARK_COLOR, PS_SEAM_TINT * sv.x + PS_HAIR_TINT * sv.y);
  }
  return vec4(col, 1.);
}
