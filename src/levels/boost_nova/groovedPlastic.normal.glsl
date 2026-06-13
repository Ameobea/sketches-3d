// Closed-form relief normal for both regions. Slots: carve = GP_CARVE·slot(g)·end(a)
// ⇒ ∇carve = GP_CARVE·(end·slot'·∇g + slot·end'·∇a). Seam: carve = gpSeamCarve(b),
// b = CELL/2 − chebyshev(d) ⇒ ∇b = −∇d. All gradients are axis-aligned in UV.
// Outward normal = normalize(N + tangential(depth·∇carve)). Mirrors
// groovedPlastic.height.glsl — keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t) {
  vec2 cl, cellId;
  float d = gpSquareDist(gpProjectUV(pos, N), cl, cellId);

  vec2 gradUV;
  if (d < GP_SQ) {
    bool alongX = gpSlotsAlongX(cellId);
    float off = gpSlotOffset(alongX ? cl.y : cl.x);
    float aSgn = alongX ? cl.x : cl.y;
    float g = abs(off);
    float a = abs(aSgn);

    float tg = clamp((g - GP_HW) / GP_WALL, 0., 1.);
    float slot = 1. - tg * tg * (3. - 2. * tg);
    float dSlot = -6. * tg * (1. - tg) / GP_WALL * sign(off);

    float te = clamp((a - GP_END_OUT + GP_END_WALL) / GP_END_WALL, 0., 1.);
    float endMask = 1. - te * te * (3. - 2. * te);
    float dEnd = -6. * te * (1. - te) / GP_END_WALL * sign(aSgn);

    vec2 perpDir = alongX ? vec2(0., 1.) : vec2(1., 0.);
    vec2 alongDir = alongX ? vec2(1., 0.) : vec2(0., 1.);
    gradUV = GP_CARVE * (endMask * dSlot * perpDir + slot * dEnd * alongDir);
  } else {
    float b = 0.5 * GP_CELL - d;
    if (b >= GP_SEAM_W) {
      return N;
    }
    float tc = b / GP_SEAM_W;
    float dCarveDb = -6. * GP_SEAM_DEPTH * tc * (1. - tc) / GP_SEAM_W;
    float th = clamp((b - GP_HAIR_HW) / GP_HAIR_WALL, 0., 1.);
    dCarveDb -= 6. * GP_HAIR_DEPTH * th * (1. - th) / GP_HAIR_WALL;
    vec2 chebDir = abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
    gradUV = -dCarveDb * chebDir;
  }

  vec3 na = abs(N);
  vec3 gradW;
  if (na.y >= na.x && na.y >= na.z) {
    gradW = vec3(gradUV.x, 0., gradUV.y);
  } else if (na.x >= na.z) {
    gradW = vec3(0., gradUV.y, gradUV.x);
  } else {
    gradW = vec3(gradUV.x, gradUV.y, 0.);
  }

  vec3 gw = depth * gradW;
  return normalize(N + (gw - dot(gw, N) * N));
}
