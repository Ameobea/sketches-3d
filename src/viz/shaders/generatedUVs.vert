// Given the current `pos` of the fragment, determines the UV coordinates by looking at
// the normal stored in `vNormalAbsolute`.  Picks the axis (x, y, or z) that the current
// triangle is most closely aligned with, and uses that axis to determine the UV
// coordinates.

vec2 generateUV(vec3 pos, vec3 normal) {
  vec3 absNormal = abs(normal);
  vec2 uv = vec2(0., 0.);

  if (absNormal.x > absNormal.y) {
    if (absNormal.x > absNormal.z) {
      uv = vec2(pos.y, pos.z);
    } else {
      uv = vec2(pos.x, pos.y);
    }
  } else {
    if (absNormal.y > absNormal.z) {
      uv = vec2(pos.x, pos.z);
    } else {
      uv = vec2(pos.x, pos.y);
    }
  }

  return uv;
}
