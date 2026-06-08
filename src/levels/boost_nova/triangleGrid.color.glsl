// Region albedo for the triangle-grid material; cast shadow + AO live in
// triangleGrid.attenuation.glsl. `pos` is the POM-displaced hit, so the edge
// distance also separates carved wall from floor. Oversampled
// (`antialiasColorShader`); the smoothsteps keep boundaries crisp at distance.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  float ed = triEdgeDist(triProjectUV(pos, vWorldNormal));
  float aa = max(ctx.unitsPerPx, 1e-4);

  // TRI_WALL_BAND_PAD insets the wall color band so POM hit imprecision can't bleed it past the band.
  float wallColStart = TRI_BORDER_END + TRI_WALL_BAND_PAD;
  float wallColEnd   = TRI_WALL_END - TRI_WALL_BAND_PAD;

  vec3 col = TRI_BG_COLOR;
  col = mix(col, TRI_BORDER_COLOR,                 smoothstep(TRI_GAP_HALF - aa, TRI_GAP_HALF + aa, ed));
  col = mix(col, TRI_FILL_COLOR * TRI_WALL_DARKEN, smoothstep(wallColStart - aa, wallColStart + aa, ed));
  col = mix(col, TRI_FILL_COLOR,                   smoothstep(wallColEnd - aa, wallColEnd + aa, ed));

  return vec4(col, 1.);
}
