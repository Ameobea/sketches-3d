// Deepest feature wins (pits and grooves don't overlap in a sane config); flat (0)
// over the padding, where the marcher terminates on its first sample.
float getPomHeight(vec3 pos, vec3 normal, float curTimeSeconds) {
  vec2 p = ssCellLocal(ssProjectUV(pos, normal));
  float c = ssCarveVS(ssShapeSdf(p), SS_DEPTH).x;
#if SS_GROOVE
  c = max(c, ssCarveVS(ssGrooveSdf(p.y), SS_GROOVE_DEPTH).x);
#endif
  return c;
}
