// Weathered-metal sheen: patina patches go matte, fine grain breaks up the
// face, seams porous-rough. Seam mix filters against a doubled footprint —
// specular aliases before albedo does.
float getCustomRoughness(vec3 pos, vec3 normal, float baseRoughness, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId, dir;
  float b = cuDist(patProjectUV(pos, vWorldNormal), cl, cellId, dir);
  vec2 paa = patAA();
  vec4 s = cuSurf(cl, cellId, b, 0.5 * (paa.x + paa.y));
  float r = mix(CU_ROUGH_TILE, CU_ROUGH_PATINA, s.x) + CU_ROUGH_GRAIN * (2. * s.y - 1.);
  return mix(r, CU_ROUGH_SEAM, cuSeamVis(b, 2. * cuDirAA(dir)));
}
