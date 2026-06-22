// Relief normal of the pit wall. carve = carve(d), d = pgHoleSdf(p) ⇒ ∇carve = (dcarve/dd)·∇d.
// ∇d (unit-ish, outward) is taken by a 2-tap central difference, uniform across all hole shapes —
// cheaper to keep correct than four hand-derived gradients, and this runs once per fragment.
vec3 getPomNormal(vec3 pos, vec3 N, float depth, float t, float aa) {
  int axis = domAxis(N);
  vec2 p = pgCellLocal(domProject(pos, axis));
  float d = pgHoleSdf(p);
  if (abs(d) >= PG_WALL_HW) {
    return N; // flat top or flat floor
  }
  float e = 0.5 * PG_WALL_HW;
  vec2 gd = vec2(pgHoleSdf(p + vec2(e, 0.)) - pgHoleSdf(p - vec2(e, 0.)),
                 pgHoleSdf(p + vec2(0., e)) - pgHoleSdf(p - vec2(0., e))) / (2. * e);
  vec2 gradUV = pgCarveVS(d).y * gd * reliefAAFade(aa, PG_WALL_HW);
  vec3 gw = depth * domUnproject(gradUV, axis);
  return normalize(N + (gw - dot(gw, N) * N));
}
