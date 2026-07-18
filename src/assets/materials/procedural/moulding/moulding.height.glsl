float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  return mdCarve(patProjectUV(pos, normal), mdKeepF());
}
