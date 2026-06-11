uniform vec3 occlusionStart; // player eye position
uniform vec3 occlusionEnd; // camera position
uniform vec4 occlusionParams; // x=revealRadius, y=revealFade, z=active(0|1), w=eyeMargin

// Closed-form 4x4 Bayer: recursive 2x2 base cell B2(a,b) = 2a + 3b - 4ab applied to the
// low/high bits. No array indexing; GLSL ES 1.00 compatible (shared with the depth material).
float getBayer4x4(vec2 p) {
  vec2 ip = mod(floor(p), 4.);
  vec2 l = mod(ip, 2.);
  vec2 h = floor(ip * 0.5);
  float b2l = 2. * l.x + 3. * l.y - 4. * l.x * l.y;
  float b2h = 2. * h.x + 3. * h.y - 4. * h.x * h.y;
  return (4. * b2l + b2h) * (1. / 16.);
}

float getTriplanarBayer(vec3 worldPos, vec3 normal, float scale) {
  vec3 blend = abs(normal);
  // saturate so the cutoffs are sharper
  blend = pow(blend, vec3(2.5));
  blend /= dot(blend, vec3(1.0));

  float ditherX = getBayer4x4(worldPos.yz * scale);
  float ditherY = getBayer4x4(worldPos.xz * scale);
  float ditherZ = getBayer4x4(worldPos.xy * scale);

  return ditherX * blend.x + ditherY * blend.y + ditherZ * blend.z;
}
