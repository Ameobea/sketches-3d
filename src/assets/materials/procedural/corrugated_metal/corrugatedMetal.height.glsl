float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  return cmCarve(patProjectUV(pos, normal).x);
}
