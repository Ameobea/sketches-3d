uniform vec3 occlusionStart;     // player eye position
uniform vec3 occlusionEnd;       // camera position
uniform vec4 occlusionParams;    // x=revealRadius, y=revealFade, z=active(0|1), w=eyeMargin

float getBayer4x4(vec2 p) {
  ivec2 ip = ivec2(mod(floor(p), 4.0));
  int index = ip.y * 4 + ip.x;
  float m[16] = float[](
     0.0/16.0,  8.0/16.0,  2.0/16.0, 10.0/16.0,
    12.0/16.0,  4.0/16.0, 14.0/16.0,  6.0/16.0,
     3.0/16.0, 11.0/16.0,  1.0/16.0,  9.0/16.0,
    15.0/16.0,  7.0/16.0, 13.0/16.0,  5.0/16.0
  );
  return m[index];
}

float getTriplanarBayer(vec3 worldPos, vec3 normal, float scale) {
    // Calculate blending weights based on the surface normal
    vec3 blend = abs(normal);
    // saturate so the cutoffs are sharper
    blend = pow(blend, vec3(2.5));
    // Normalize so the weights sum to 1.0
    blend /= dot(blend, vec3(1.0)); 

    // Sample the 2D Bayer matrix along the three primary planes
    float ditherX = getBayer4x4(worldPos.yz * scale);
    float ditherY = getBayer4x4(worldPos.xz * scale);
    float ditherZ = getBayer4x4(worldPos.xy * scale);

    // Blend the results
    return ditherX * blend.x + ditherY * blend.y + ditherZ * blend.z;
}