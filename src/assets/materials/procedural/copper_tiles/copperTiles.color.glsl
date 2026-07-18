// Per-tile oxidized palette + independent mottling; seams and corner pits blend
// to dark oxide via footprint-filtered coverage.
vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId, dir;
  float b = cuDist(patProjectUV(pos, vWorldNormal), cl, cellId, dir);
  vec2 paa = patAA();
  float aaIso = 0.5 * (paa.x + paa.y);
  vec4 tp = cuTileParams(cellId, aaIso);
  vec4 s = cuSurf(cl, cellId, b, aaIso);
  vec3 col = mix(CU_COL_A, CU_COL_B, smoothstep(0.2, 0.8, tp.x));
  col = mix(col, CU_PATINA_COL, s.x * CU_PATINA_MAX);
  col *= mix(CU_VAL_MIN, CU_VAL_MAX, tp.z)
       + CU_GRAIN_AMP * (2. * s.y - 1.)
       + CU_BLOTCH_VAL * (2. * s.w - 1.);
  col *= mix(1., CU_RIM_DARK, s.z);
  col = mix(col, CU_SEAM_COL, cuSeamVis(b, cuDirAA(dir)));
  return vec4(col, 1.);
}
