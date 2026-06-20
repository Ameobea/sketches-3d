// Round pits carved into the swept strip, keyed to the mesh's analytic UV (rail_sweep) and driven
// by tangent-space POM. `pomUvWorldScale()` converts the UV (U = arc length, V = profile param) to
// world units, so the pits stay circular regardless of the profile's V parameterization. Color +
// attenuation read the marched UV through `pomMeshUv(pos)`; the height shader carves `dgGap`.
const float DG_PIT_PITCH  = 1.2;   // pit grid spacing (world units)
const float DG_PIT_RADIUS = 0.34;  // pit radius (world units)
const float DG_WALL       = 0.06;  // pit-wall ramp half-width (world units)
const vec3  DG_BASE_COLOR = vec3(0.12, 0.13, 0.14);
const vec3  DG_GAP_COLOR  = vec3(0.012, 0.014, 0.016);
const float DG_AO_GAP     = 0.4;
const float DG_DIRECT_GAP = 0.45;

// Carve coverage 0..1 (1 = inside a pit), world-isotropic and AA-widened by the footprint `aa`.
// float dgGap(vec2 uv, float aa) {
//   float w = max(DG_WALL, aa);
//   vec2 wpos = uv * pomUvWorldScale();                         // UV → world units (keeps pits round)
//   vec2 d = (fract(wpos / DG_PIT_PITCH) - 0.5) * DG_PIT_PITCH; // world offset to nearest pit center
//   return 1.0 - smoothstep(DG_PIT_RADIUS - w, DG_PIT_RADIUS + w, length(d));
// }

const float DG_SLAT_PITCH = 1.5;
const float DG_GAP_HW = 0.16;

// Signed offset to the nearest gap centerline along U.
float dgGapOffset(vec2 uv) {
  return (fract(uv.x / DG_SLAT_PITCH + 0.5) - 0.5) * DG_SLAT_PITCH;
}

float dgGap(vec2 uv, float aa) {
  // if (fract(uv.y * 6.) > 0.5) {
  //   return 0.;
  // }
  float w = max(DG_WALL, aa);
  float g = abs(dgGapOffset(uv));
  float vHeight = smoothstep(0.5-w, 0.5+w, sin(fract(uv.y * 6.) * 3.14159));
  g = min(g, vHeight); // add horizontal grooves keyed to V
  return 1.0 - smoothstep(DG_GAP_HW - w, DG_GAP_HW + w, g);
}
