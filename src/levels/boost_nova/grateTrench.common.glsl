// Drainage-gutter trench material: infinite trench strips (periodic across
// one axis, unbounded along the other) bridged by slats, with deep dark gaps
// carved between them and a solid rail along each trench edge. Carving is
// zero outside the trenches, so the marcher's first sample terminates over
// most of the surface.

const bool GT_ALONG_X = true; // trenches run along the projected-UV x axis

const float GT_PITCH      = 6.;    // spacing between trench centerlines
const float GT_HALF_W     = 0.9;   // trench half-width
const float GT_SLAT_PITCH = 0.5;   // slat repeat along the trench
const float GT_GAP_HW     = 0.13;  // gap floor half-width (slat thickness = pitch − 2·(hw+wall))
const float GT_WALL       = 0.018; // gap wall ramp width
const float GT_END        = 0.1;   // gap end inset from the trench edge (the rail)
const float GT_END_WALL   = 0.05;  // gap end-wall ramp width
const float GT_END_OUT    = GT_HALF_W - GT_END;
const float GT_CARVE      = 0.8;   // marcher clamps carved depth at 0.8 = full pom.depth

const vec3 GT_BASE_COLOR = vec3(0.106, 0.110, 0.117);
const vec3 GT_VOID_COLOR = vec3(0.006, 0.007, 0.008); // reads as a hole, darker than the grate slots

const float GT_AO_VOID     = 0.45;
const float GT_DIRECT_VOID = 0.3;

// Dominant-axis projection into 2D (Y→xz, X→zy, Z→xy), matching the other POM materials.
vec2 gtProjectUV(vec3 pos, vec3 axisNormal) {
  vec3 a = abs(axisNormal);
  if (a.y >= a.x && a.y >= a.z) {
    return pos.xz;
  } else if (a.x >= a.z) {
    return vec2(pos.z, pos.y);
  }
  return vec2(pos.x, pos.y);
}

// Signed offset to the nearest trench centerline (across-axis).
float gtTrenchOffset(vec2 uv) {
  float v = GT_ALONG_X ? uv.y : uv.x;
  return (fract(v / GT_PITCH + 0.5) - 0.5) * GT_PITCH;
}

// Signed offset to the nearest gap centerline (along-axis).
float gtGapOffset(vec2 uv) {
  float u = GT_ALONG_X ? uv.x : uv.y;
  return (fract(u / GT_SLAT_PITCH + 0.5) - 0.5) * GT_SLAT_PITCH;
}

// AA'd visual carve (gap coverage) for the color + attenuation slots; `dv`
// must already be inside the trench.
float gtVisCarve(vec2 uv, float dv, float aa) {
  float g = abs(gtGapOffset(uv));
  float w = max(0.5 * GT_WALL, aa);
  float we = max(0.5 * GT_END_WALL, aa);
  return (1. - smoothstep(GT_GAP_HW + 0.5 * GT_WALL - w, GT_GAP_HW + 0.5 * GT_WALL + w, g))
       * (1. - smoothstep(GT_END_OUT - 0.5 * GT_END_WALL - we, GT_END_OUT - 0.5 * GT_END_WALL + we, dv));
}
