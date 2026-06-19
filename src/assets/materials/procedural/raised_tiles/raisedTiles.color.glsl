// Mottled concrete: base albedo modulated by large-scale staining + fine grain
// (grain dissolved with footprint), a per-tile value jitter, and a faint hairline
// seam painted at every cell boundary. Swap RT_CONCRETE/RT_SEAM_COLOR for a
// ceramic look.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 uv = rtProjectUV(pos, vWorldNormal);
  vec2 cellId, cl, edgeDir;
  float b = rtCellField(uv, cellId, cl, edgeDir);
  float aa = max(ctx.aaFootprint, 1e-4);

  // Retain only a fraction of the per-tile/seam cues once the relief is gone, so
  // distant flat tiles don't read as differently-shaded squares.
  float keep = mix(1.0, 0.3, rtPomFade(ctx.distanceToCamera));

  vec3 col = RT_CONCRETE;
  float stainBroad = fbm(uv * 0.16) - 0.5;
  float stainMed = fbm(uv * 0.7 + 13.0) - 0.5;
  float grain = fadeToMean(fbm(uv * 3.2), 0.5, aa, 0.55) - 0.5;
  col *= 1.0 + RT_MOTTLE_AMP * (2.2 * stainBroad + 1.1 * stainMed) + 0.08 * grain;
  col *= 1.0 + RT_TINT_AMP * keep * (hash(cellId + 11.7) - 0.5) * 2.0;
  col *= 1.0 - 0.13 * keep * smoothstep(0.68, 1.0, hash(cellId + 4.2));

  float seam = 1.0 - smoothstep(RT_SEAM_HW, RT_SEAM_HW + max(aa, 0.006), b);
  seam = aaThinFeature(seam, aa, RT_FADE_PERIOD);
  col = mix(col, RT_SEAM_COLOR, RT_SEAM_DARK * keep * seam);
  return vec4(max(col, 0.0), 1.0);
}
