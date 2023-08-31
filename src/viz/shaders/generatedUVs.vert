// Given the current `pos` of the fragment, determines the UV coordinates by looking at
// the normal stored in `vNormalAbsolute`.  Picks the axis that the current triangle is
// most closely aligned with, and uses that axis to determine the UV coordinates.

vec3 generateTriplanarWeights(vec3 normal) {
  vec3 weights = abs(normal);
  weights = normalize(max(weights, 0.0001)); // Avoid divide by zero
  weights = weights / (weights.x + weights.y + weights.z);
  return weights;
}

vec2 generateUV(vec3 pos, vec3 normal) {
  vec3 weights = generateTriplanarWeights(normal);

  if (weights.x >= weights.y && weights.x >= weights.z) {
    return pos.yz;
  } else if (weights.y >= weights.x && weights.y >= weights.z) {
    return pos.xz;
  } else {
    return pos.xy;
  }
}
