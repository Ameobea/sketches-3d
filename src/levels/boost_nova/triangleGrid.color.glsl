// Region albedo for the triangle-grid material; cast shadow + AO live in
// triangleGrid.attenuation.glsl. `pos` is the POM-displaced hit, so the edge
// distance also separates carved wall from floor. Footprint-AA'd bands (aaStep)
// that dissolve toward the fill color at distance so the grid stops aliasing.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float ed = triEdgeDist(domProject(pos, domAxis(vWorldNormal)));
  float aa = max(ctx.aaFootprint, 1e-4);

  // TRI_WALL_BAND_PAD insets the wall color band so POM hit imprecision can't bleed it past the band.
  float wallColStart = TRI_BORDER_END + TRI_WALL_BAND_PAD;
  float wallColEnd   = TRI_WALL_END - TRI_WALL_BAND_PAD;

  vec3 col = TRI_BG_COLOR;
  col = mix(col, TRI_BORDER_COLOR,                 aaStep(TRI_GAP_HALF, ed, aa));
  col = mix(col, TRI_FILL_COLOR * TRI_WALL_DARKEN, aaStep(wallColStart, ed, aa));
  col = mix(col, TRI_FILL_COLOR,                   aaStep(wallColEnd, ed, aa));

  // Border/seam bands are thin vs the triangle; dissolve them to the dominant
  // fill once the footprint outgrows the band region, so the grid stops aliasing.
  col = mix(col, TRI_FILL_COLOR, fadeToMeanFactor(aa, TRI_FADE_PERIOD));

  return vec4(col, 1.);
}
