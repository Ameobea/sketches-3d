// projectedField height: flat ridge tops (carve 0 → first-sample early-out) with each crevice
// carved by the shared chCarveVS profile. See chevronStrips.common.glsl.
float gridHeight(vec2 uv, float t) {
  return chCarveVS(abs(chWOff(uv))).x;
}
