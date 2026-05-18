// Companion to masonryTiles.height.glsl. Recomputes the tile mask, dent, and
// groove zone at the displaced hit position to drive pseudo-AO darkening.
//
// Uses the new ctx fields:
//   ctx.vWorldPos      — base-mesh world position (per-fragment constant);
//                        gives a stable AA band matching the height shader.
//   ctx.vWorldNormal   — base-mesh normal; stable axis selection (the `normal`
//                        param is the POM-perturbed normal and tilts at rims).
// `pos` is the displaced hit position — used for the actual pattern lookup so
// the AO follows the rendered surface, not the flat base mesh.

// Debug visualizations (set to 0 to disable, otherwise pick a mode):
//   1 = projection-axis pick (R=X-wall, G=Y-horiz, B=Z-wall). Hard color
//       change across the shimmer line == projection seam confirmed.
//   2 = ctx.vWorldNormal as RGB. Noisy / non-constant on a single face ==
//       vertex normals are being smoothed across the corner edge and POM
//       is interpolating between two perpendicular base normals.
//   3 = POM-perturbed `normal` as RGB. Differs from mode 2 at rims/bevels.
//   4 = combined triplanar × tile-breaking diffuse-tap count.
//       1=green, 3=yellow, 6=orange, 9=red. REQUIRES `useTriplanarMapping:
//       true` on the material — calling getCombinedTriplanarTapCount
//       without triplanar will fail to compile.
#define DEBUG_MODE 0

vec4 getFragColor(vec3 baseColor, vec3 pos, vec3 normal, float curTimeSeconds, SceneCtx ctx) {
  vec3 absN = abs(ctx.vWorldNormal);
#if DEBUG_MODE == 1
  vec3 dbg = (absN.y >= absN.x && absN.y >= absN.z) ? vec3(0.0, 1.0, 0.0)
           : (absN.x >= absN.z)                     ? vec3(1.0, 0.0, 0.0)
           :                                          vec3(0.0, 0.0, 1.0);
  return vec4(dbg, 1.0);
#elif DEBUG_MODE == 2
  return vec4(ctx.vWorldNormal * 0.5 + 0.5, 1.0);
#elif DEBUG_MODE == 3
  return vec4(normal * 0.5 + 0.5, 1.0);
#elif DEBUG_MODE == 4
  // Combined diffuse-tap cost: sum of active hex taps across active triplanar
  // axes (the hex-breaking shader's own skip threshold is honored). 1 = cheap
  // (axis-aligned face, single dominant hex sample); 9 = worst-case (all 3
  // triplanar axes contributing × all 3 hex samples per axis active).
  // Color ramp: green → yellow → orange → red.
  float taps = getCombinedTriplanarTapCount(pos, normal, vec2(uvTransform[0][0], uvTransform[1][1]));
  vec3 dbg = (taps <= 3.0)
    ? mix(vec3(0.2, 1.0, 0.2), vec3(1.0, 1.0, 0.2), (taps - 1.0) * 0.5)
    : mix(vec3(1.0, 1.0, 0.2), vec3(1.0, 0.2, 0.2), (taps - 3.0) / 6.0);
  return vec4(dbg, 1.0);
#endif

  vec2 uv;
  if (absN.y >= absN.x && absN.y >= absN.z) uv = pos.xz;
  else if (absN.x >= absN.z) uv = vec2(pos.z, pos.y);
  else uv = vec2(pos.x, pos.y);

  const vec2  CELL = vec2(3.2, 1.8);
  const vec2  GROOVE = vec2(0.16, 0.16);
  const vec2  TILE_HALF = 0.5 * (CELL - GROOVE);
  const float CORNER = 0.08;
  const float BEVEL = 0.15;
  const float GROOVE_DEPTH = 0.65;
  const float DENT_DEPTH = 0.4;

  float row = floor(uv.y / CELL.y + 0.5);
  float rowHash = fract(sin(row * 12.9898 + 78.233) * 43758.5453);
  vec2 uvStagger = vec2(uv.x - rowHash * CELL.x, uv.y);
  vec2 cellLocal = (fract(uvStagger / CELL + 0.5) - 0.5) * CELL;

  vec2 q = abs(cellLocal) - TILE_HALF + CORNER;
  float sdf = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - CORNER;

  float band = max(BEVEL, distance(ctx.cameraPosition, ctx.vWorldPos) * 0.001);
  float dentZone = 1.0 - smoothstep(-band, 0.0, sdf);
  float grooveZone = smoothstep(0.0, band, sdf);

  vec2 tileLocal = cellLocal / TILE_HALF;
  float bowl = max(0.0, cos(tileLocal.x * 1.5707963) * cos(tileLocal.y * 1.5707963));
  // Decompose so we can apply per-brick variation to the tile contribution
  // only. The groove must stay uniform across bricks — otherwise the midline
  // between two cells, where the per-brick hash flips, would read as a
  // visible seam down the middle of every groove.
  float tileH = bowl * DENT_DEPTH * dentZone;
  float grooveH = GROOVE_DEPTH * grooveZone;

  // Per-brick AO variation: ±30% scale on the dent darkening only. Indexed
  // by the staggered (col, row) pair, so adjacent rows pick from independent
  // random sets after the row offset has shifted them.
  float col = floor(uvStagger.x / CELL.x + 0.5);
  float brickHash = fract(sin(dot(vec2(col, row), vec2(127.1, 311.7))) * 43758.5453);
  float brickAo = 1.0 + (brickHash - 0.5) * 0.6;

  // Distance-driven AO fade: full strength near the camera, smoothing to a
  // 20% floor at 250 world units so the shading carries weight up close
  // without darkening distant walls into mush.
  float dist = distance(ctx.cameraPosition, ctx.vWorldPos);
  float distFade = 1.0 - 0.8 * smoothstep(0.0, 250.0, dist);

  // Pseudo-AO. Aggressive depth-driven darkening + an extra groove pass for
  // the narrow recess; per-brick variation applies to the dent only.
  float darkAmt = (tileH * brickAo * 1.55 + grooveH * 3.7 + grooveZone * 0.4) * distFade;
  float ao = clamp(1.0 - darkAmt, 0.1, 1.0);

  return vec4(baseColor * ao, 1.0);
}
