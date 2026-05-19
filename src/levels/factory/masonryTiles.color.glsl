// Companion to masonryTiles.height.glsl. Recomputes the tile decomposition at
// the displaced hit (`pos`) via the shared `evalMasonry` to drive pseudo-AO.
// Axis selection uses ctx.vWorldNormal (stable base-mesh normal) rather than
// the POM-perturbed `normal`, which tilts at rims.
//
// Debug visualizations (set to 0 to disable):
//   1 = projection-axis pick (R=X-wall, G=Y-horiz, B=Z-wall)
//   2 = ctx.vWorldNormal as RGB
//   3 = POM-perturbed `normal` as RGB
//   4 = combined triplanar × tile-breaking diffuse-tap count (REQUIRES
//       useTriplanarMapping:true; otherwise fails to compile)
#define DEBUG_MODE 0

vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
#if DEBUG_MODE == 1
  vec3 absN = abs(ctx.vWorldNormal);
  vec3 dbg = (absN.y >= absN.x && absN.y >= absN.z) ? vec3(0., 1., 0.)
    : (absN.x >= absN.z) ? vec3(1., 0., 0.)
    : vec3(0., 0., 1.);
  return vec4(dbg, 1.);
#elif DEBUG_MODE == 2
  return vec4(ctx.vWorldNormal * 0.5 + 0.5, 1.);
#elif DEBUG_MODE == 3
  return vec4(normal * 0.5 + 0.5, 1.);
#elif DEBUG_MODE == 4
  float taps = getCombinedTriplanarTapCount(pos, normal, vec2(uvTransform[0][0], uvTransform[1][1]));
  vec3 dbg = (taps <= 3.0)
    ? mix(vec3(0.2, 1., 0.2), vec3(1., 1., 0.2), (taps - 1.) * 0.5)
    : mix(vec3(1., 1., 0.2), vec3(1., 0.2, 0.2), (taps - 3.) / 6.);
  return vec4(dbg, 1.);
#endif

  float dist = distance(ctx.cameraPosition, ctx.vWorldPos);
  Masonry m = evalMasonry(pos, ctx.vWorldNormal, dist);

  const float GROOVE_DEPTH = 0.65;
  const float DENT_DEPTH = 0.4;
  float tileH = m.bowl * DENT_DEPTH * m.dentZone;
  float grooveH = GROOVE_DEPTH * m.grooveZone;

  // small randomization of fake-AO intensity for each brick
  float brickHash = fract(sin(dot(m.cellId, vec2(127.1, 311.7))) * 43758.5453);
  float brickAo = 1. + (brickHash - 0.5) * 0.6;

  // fade out fake-AO intensity as distance to camera increases
  float distFade = 1. - 0.8 * smoothstep(0., 250., dist);

  float darkAmt = (tileH * brickAo * 1.55 + grooveH * 3.7 + m.grooveZone * 0.4) * distFade;
  float ao = clamp(1. - darkAmt, 0.1, 1.);

  return vec4(baseColor * ao, 1.);
}
