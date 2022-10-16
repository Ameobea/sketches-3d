// Given the current `pos` of the fragment, determines the UV coordinates by looking at
// the normal stored in `vNormalAbsolute`.  Picks the axis that the current triangle is
// most closely aligned with, and uses that axis to determine the UV coordinates.

vec2 generateUV(vec3 pos, vec3 normal) {
  vec3 absNormal = abs(normal);
  vec2 uv = vec2(0.);

  // if (absNormal.x > absNormal.y && absNormal.x > absNormal.z) {
  //   // project along the x-axis
  //   uv = (normal.x > 0.0) ? pos.yz : pos.zy;
  // } else if (absNormal.z > absNormal.x && absNormal.z > absNormal.y) {
  //   // project along the z-axis
  //   uv = (normal.z > 0.0) ? pos.xy : pos.yx;
  // } else {
  //   // project along the y-axis
  //   uv = (normal.y > 0.0) ? pos.zx : pos.xz;
  // }

  if (absNormal.x > absNormal.y && absNormal.x > absNormal.z) {
    // project along the x-axis
    uv = pos.yz;
  } else if (absNormal.z > absNormal.x && absNormal.z > absNormal.y) {
    // project along the z-axis
    uv = pos.xy;
  } else {
    // project along the y-axis
    uv = pos.zx;
  }

  return uv;
}
