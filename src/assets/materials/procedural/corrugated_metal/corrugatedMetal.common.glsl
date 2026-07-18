// Painted corrugated sheet metal: an infinite 1D corrugation — wide, gently
// crowned ribs split by narrow carved grooves, every CM_LAP_EVERY-th groove a
// deeper/wider panel lap joint. Dual-mode pattern space (cf. moulding):
// world-space dominant-axis projection, or mesh UVs for rail_sweep meshes
// (U = spine arc length in world units, V = ring param wrapping [0,1)).
// PAT_AXIS picks which pattern coordinate carries the corrugation, so pipe
// grooves either wrap the rings (0) or run down the span (1). Crest crowning is
// shading-only (relief normal), keeping the height field 0 on crests so the
// marcher first-sample-terminates outside grooves.

#ifndef CM_PITCH
#define CM_PITCH 0.55 // groove repeat
#endif
#ifndef CM_HW
#define CM_HW 0.05 // groove floor half-width
#endif
#ifndef CM_WALL
#define CM_WALL 0.09 // groove wall ramp width
#endif
#ifndef CM_CARVE
#define CM_CARVE 0.6 // groove depth (fraction of pom.depth)
#endif
#ifndef CM_LAP_EVERY
#define CM_LAP_EVERY 7 // every Nth groove is a panel lap joint
#endif
#ifndef CM_LAP_HW
#define CM_LAP_HW 0.085
#endif
#ifndef CM_LAP_WALL
#define CM_LAP_WALL 0.12
#endif
#ifndef CM_LAP_CARVE
#define CM_LAP_CARVE 0.85
#endif
#ifndef CM_CROWN
#define CM_CROWN 0.12 // shading-only rib crowning amplitude (carve units)
#endif
#ifndef CM_COLOR
#define CM_COLOR vec3(0.072, 0.067, 0.078)
#endif
#ifndef CM_COLOR_DEEP
#define CM_COLOR_DEEP vec3(0.030, 0.028, 0.034) // groove-floor albedo (paint pooling/shadowed)
#endif
#ifndef CM_STREAK_FREQ
#define CM_STREAK_FREQ vec2(1.6, 0.22) // wear-streak fbm freq (low v = streaks run along grooves)
#endif
#ifndef CM_STREAK_AMP
#define CM_STREAK_AMP 0.08
#endif
#ifndef CM_ROUGH_TOP
#define CM_ROUGH_TOP 0.5
#endif
#ifndef CM_ROUGH_DEEP
#define CM_ROUGH_DEEP 0.68
#endif
#ifndef CM_ROUGH_STREAK
#define CM_ROUGH_STREAK 0.08 // worn-sheen roughness variation from the streak field
#endif
#ifndef CM_AO_DEEP
#define CM_AO_DEEP 0.45 // indirect mul at full carve
#endif
#ifndef CM_DIRECT_DEEP
#define CM_DIRECT_DEEP 0.62 // direct mul at full carve; kills specular punch-through
#endif

// Signed offset to the nearest groove centerline + whether it's a lap joint.
float cmGroove(float s, out bool lap) {
  float k = floor(s / CM_PITCH + 0.5);
  lap = mod(k, float(CM_LAP_EVERY)) < 0.5;
  return s - k * CM_PITCH;
}

// (floor half-width, wall ramp width, carve depth) for a groove vs a lap joint.
vec3 cmParams(bool lap) {
  return lap ? vec3(CM_LAP_HW, CM_LAP_WALL, CM_LAP_CARVE) : vec3(CM_HW, CM_WALL, CM_CARVE);
}

// Carve only — the marcher's hot path, no gradient math. 0 on the rib crests.
float cmCarve(float s) {
  bool lap;
  float g = abs(cmGroove(s, lap));
  vec3 gp = cmParams(lap);
  return gp.z * (1. - smoothstep(gp.x, gp.x + gp.y, g));
}

// AA'd visual groove coverage shared by color/roughness/attenuation; lap joints
// read proportionally darker via their deeper carve.
float cmCov(float s, float aa) {
  bool lap;
  float g = abs(cmGroove(s, lap));
  vec3 gp = cmParams(lap);
  return (gp.z / CM_LAP_CARVE) * aaSlot(g, CM_PITCH, gp.x, gp.y, aa);
}

// Anisotropic wear-streak fbm, streaks running along the grooves. In UV mode the
// v-wrap coordinate is embedded on a cylinder whose arc length matches the planar
// domain span, so the noise is seam-free around the wrap (cf. moulding).
float cmStreak(vec2 p) {
#if PAT_UV_MODE == 1
  float ang = 6.2832 * vUv.y;
#if PAT_AXIS == 0
  float rF = CM_STREAK_FREQ.y;
  float xF = CM_STREAK_FREQ.x;
#else
  float rF = CM_STREAK_FREQ.x;
  float xF = CM_STREAK_FREQ.y;
#endif
  float r = PAT_UV_SCALE.y * rF / 6.2832;
  return fbm(vec3(vUv.x * PAT_UV_SCALE.x * xF, cos(ang) * r, sin(ang) * r));
#else
  return fbm(p * CM_STREAK_FREQ);
#endif
}

// Faded, centered streak signal shared by the color + roughness slots: the fbm
// (the material's costliest op) is skipped entirely once the fade zeroes it, and
// cached per fragment across the two slots.
float cmStreakCache = -1e9;
float cmStreakSignal(vec2 p, vec2 aa) {
  float w = 1. - fadeToMeanFactor(max(aa.x * CM_STREAK_FREQ.x, aa.y * CM_STREAK_FREQ.y), 1.5);
  if (w <= 0.) {
    return 0.;
  }
  if (cmStreakCache < -1e8) {
    cmStreakCache = cmStreak(p) - 0.5;
  }
  return w * cmStreakCache;
}
