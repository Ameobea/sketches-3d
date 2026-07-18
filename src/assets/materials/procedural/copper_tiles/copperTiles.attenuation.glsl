// Seam-carve dimming: indirect (fake AO in the valleys/pits) + direct (kills
// specular punch-through). Too shallow for a cast shadow to read.
vec2 getLightAttenuation(vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec2 cl, cellId, dir;
  float b = cuDist(patProjectUV(pos, vWorldNormal), cl, cellId, dir);
  float sv = cuSeamVis(b, cuDirAA(dir));
  return vec2(mix(1., CU_DIRECT_SEAM, sv), mix(1., CU_AO_SEAM, sv));
}
