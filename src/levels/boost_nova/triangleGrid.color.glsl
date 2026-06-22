// Region albedo; cast shadow + AO live in triangleGrid.attenuation.glsl. Reads the shared
// at-hit frame (TriHit). Footprint-AA'd bands (aaStep) that dissolve to fill at distance so
// the grid stops aliasing.
vec4 gridColor(TriHit h, vec3 baseColor, SceneCtx ctx) {
  float ed = min(h.d.x, min(h.d.y, h.d.z));
  float aa = max(ctx.aaFootprint, 1e-4);

  // TRI_WALL_BAND_PAD insets the wall color band so POM hit imprecision can't bleed it past the band.
  float wallColStart = TRI_BORDER_END + TRI_WALL_BAND_PAD;
  float wallColEnd = TRI_WALL_END - TRI_WALL_BAND_PAD;

  vec3 col = TRI_BG_COLOR;
  col = mix(col, TRI_BORDER_COLOR, aaStep(TRI_GAP_HALF, ed, aa));
  col = mix(col, TRI_FILL_COLOR * TRI_WALL_DARKEN, aaStep(wallColStart, ed, aa));
  col = mix(col, TRI_FILL_COLOR, aaStep(wallColEnd, ed, aa));

  // Dissolve the thin border/seam bands to the dominant fill once the footprint outgrows them.
  col = mix(col, TRI_FILL_COLOR, fadeToMeanFactor(aa, TRI_FADE_PERIOD));

  return vec4(col, 1.);
}
