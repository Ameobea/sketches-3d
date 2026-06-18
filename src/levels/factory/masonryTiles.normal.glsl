// Closed-form POM floor normal for the masonry height field. Replaces the
// engine's finite-difference `pomAnalyticNormal` (which costs 3 extra
// `getPomHeight` taps) with a single analytic evaluation: the gradient of the
// carved depth in the tile UV plane, lifted into world space.
//
// The decomposition mirrors `evalMasonryCore` (masonryTiles.common.glsl) — keep
// the two in sync. This runs once per fragment (not in the march loop), so it
// recomputes the cell decomposition rather than bloating the hot-path struct.
//
// Result is consistent with the engine's finite-difference form:
//   surface = base - H*N  (H = depth * carvedDepth, carved inward along -N)
//   outward normal = normalize(N + tangential(gradient of H))
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec3 absN = abs(N);
  vec2 uv;
  if (absN.y >= absN.x && absN.y >= absN.z) {
    uv = pos.xz;
  } else if (absN.x >= absN.z) {
    uv = vec2(pos.z, pos.y);
  } else {
    uv = vec2(pos.x, pos.y);
  }

  float row = floor(uv.y / MASONRY_CELL.y + 0.5);
  float rowHash = masonryHash11(row);
  vec2 uvStagger = vec2(uv.x - rowHash * MASONRY_CELL.x, uv.y);
  vec2 cellLocal = (fract(uvStagger / MASONRY_CELL + 0.5) - 0.5) * MASONRY_CELL;

  vec2 q = abs(cellLocal) - MASONRY_TILE_HALF + MASONRY_CORNER_RADIUS;
  float sdf = length(max(q, 0.)) + min(max(q.x, q.y), 0.) - MASONRY_CORNER_RADIUS;
  float band = max(MASONRY_BEVEL, aa);

  float dentZone = 1. - smoothstep(-band, 0., sdf);
  float grooveZone = smoothstep(0., band, sdf);
  vec2 tileLocal = cellLocal / MASONRY_TILE_HALF;
  vec2 bowlXY = max(vec2(0.), 1. - tileLocal * tileLocal);
  float bowl = bowlXY.x * bowlXY.y;

  // --- analytic gradient of carved depth w.r.t. uv (== w.r.t. cellLocal, since
  //     the row stagger and `fract` cell wrap are unit-slope between seams) ---

  // d(sdf)/d(cellLocal): rounded-box SDF gradient.
  vec2 sgn = sign(cellLocal);
  vec2 dSdf;
  if (max(q.x, q.y) > 0.) {
    vec2 mq = max(q, 0.);                          // exterior: nearest-edge dir
    dSdf = (mq / max(length(mq), 1e-6)) * sgn;
  } else {
    dSdf = (q.x > q.y) ? vec2(sgn.x, 0.) : vec2(0., sgn.y); // interior: dominant axis
  }

  // d(smoothstep(e0,e1,sdf))/d(sdf) = 6 t (1-t) / (e1 - e0), width = band.
  float td = clamp((sdf + band) / band, 0., 1.);   // dentZone:  smoothstep(-band, 0)
  float tg = clamp(sdf / band, 0., 1.);            // grooveZone: smoothstep(0, band)
  vec2 dDent   = (-6. * td * (1. - td) / band) * dSdf;  // d(1 - smoothstep)
  vec2 dGroove = ( 6. * tg * (1. - tg) / band) * dSdf;

  // d(bowl)/d(cellLocal): bowl = max(0,1-tx^2) * max(0,1-ty^2).
  vec2 dBowlAxis = vec2(
    bowlXY.x > 0. ? -2. * tileLocal.x / MASONRY_TILE_HALF.x : 0.,
    bowlXY.y > 0. ? -2. * tileLocal.y / MASONRY_TILE_HALF.y : 0.
  );
  vec2 dBowl = vec2(dBowlAxis.x * bowlXY.y, bowlXY.x * dBowlAxis.y);

  // d(carvedDepth)/d(uv), carvedDepth = bowl*DENT*dentZone + GROOVE*grooveZone.
  vec2 gradUV =
      MASONRY_DENT_DEPTH * (dentZone * dBowl + bowl * dDent)
    + MASONRY_GROOVE_DEPTH * dGroove;

  // The marcher clamps carved depth to [0, 0.8]; in the clamped (flat-floor)
  // regions the gradient is zero, matching the finite-difference normal.
  float hRaw = bowl * MASONRY_DENT_DEPTH * dentZone + MASONRY_GROOVE_DEPTH * grooveZone;
  if (hRaw <= 0. || hRaw >= 0.8) {
    gradUV = vec2(0.);
  }
  gradUV *= reliefAAFade(aa, MASONRY_BEVEL);

  // Lift the uv-plane gradient back into world space via the same axis pick.
  vec3 gradW;
  if (absN.y >= absN.x && absN.y >= absN.z) {
    gradW = vec3(gradUV.x, 0., gradUV.y);          // uv = pos.xz
  } else if (absN.x >= absN.z) {
    gradW = vec3(0., gradUV.y, gradUV.x);          // uv = (pos.z, pos.y)
  } else {
    gradW = vec3(gradUV.x, gradUV.y, 0.);          // uv = (pos.x, pos.y)
  }

  vec3 g = depth * gradW;
  return normalize(N + (g - dot(g, N) * N));
}
