uniform vec3 occlusionStart; // player eye position
uniform vec3 occlusionEnd; // camera position
uniform vec4 occlusionParams; // x=revealRadius, y=revealFade, z=active(0|1), w=eyeMargin

float getBayer4x4(vec2 p) {
  ivec2 ip = ivec2(mod(floor(p), 4.0));
  int index = ip.y * 4 + ip.x;
  float m[16] = float[](
     0./16., 8./16., 2./16., 10./16.,
    12./16., 4./16., 14./16., 6./16.,
     3./16., 11./16., 1./16., 9./16.,
    15./16., 7./16., 13./16., 5./16.
  );
  return m[index];
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
