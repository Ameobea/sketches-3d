// Closed-form relief normal for both regions. Slots: carve = GP_CARVE·slot(g)·end(a)
// ⇒ ∇carve = GP_CARVE·(end·slot'·∇g + slot·end'·∇a). Seam: carve = gpSeamCarve(b),
// b = CELL/2 − chebyshev(d) ⇒ ∇b = −∇d. All gradients are axis-aligned in UV.
// Outward normal = normalize(N + tangential(depth·∇carve)). Mirrors
// groovedPlastic.height.glsl — keep in sync.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  vec2 cl, cellId;
  float d = gpSquareDist(gpProjectUV(pos, N), cl, cellId);

  vec2 gradUV;
  if (d < GP_SQ) {
    bool alongX = gpSlotsAlongX(cellId);
    float off = gpSlotOffset(alongX ? cl.y : cl.x);
    float aSgn = alongX ? cl.x : cl.y;
    float g = abs(off);
    float a = abs(aSgn);

    float slot = clamp((GP_HW + GP_WALL - g) / GP_WALL, 0., 1.);
    float dSlot = (g > GP_HW && g < GP_HW + GP_WALL) ? -sign(off) / GP_WALL : 0.;

    float endMask = clamp((GP_END_OUT - a) / GP_END_WALL, 0., 1.);
    float dEnd = (a > GP_END_OUT - GP_END_WALL && a < GP_END_OUT) ? -sign(aSgn) / GP_END_WALL : 0.;

    vec2 perpDir = alongX ? vec2(0., 1.) : vec2(1., 0.);
    vec2 alongDir = alongX ? vec2(1., 0.) : vec2(0., 1.);
    gradUV = GP_CARVE * (endMask * dSlot * perpDir + slot * dEnd * alongDir);
    gradUV *= reliefAAFade(aa, GP_WALL);
  } else {
    float b = 0.5 * GP_CELL - d;
    if (b >= GP_SEAM_W) {
      return N;
    }
    float dCarveDb = -GP_SEAM_DEPTH / GP_SEAM_W; // b is always < GP_SEAM_W in this branch
    dCarveDb += (b > GP_HAIR_HW && b < GP_HAIR_HW + GP_HAIR_WALL) ? -GP_HAIR_DEPTH / GP_HAIR_WALL : 0.;
    vec2 chebDir = abs(cl.x) >= abs(cl.y) ? vec2(sign(cl.x), 0.) : vec2(0., sign(cl.y));
    gradUV = -dCarveDb * chebDir;
    gradUV *= reliefAAFade(aa, GP_HAIR_WALL);
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
