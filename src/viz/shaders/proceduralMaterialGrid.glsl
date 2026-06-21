// Shared substrate for capability-ladder POM materials (see pom-capability-ladder-plan.md).
// The dominant-axis projection (Y→xz, X→zy, Z→xy) every grid material used to carry its own
// copy of, plus the cell decomposition and profile primitives, live here once. The marcher
// hoists the projection out of its inner loop via these; slots reuse the identical mapping.

int domAxis(vec3 n) {
  vec3 a = abs(n);
  if (a.y >= a.x && a.y >= a.z) { return 1; }
  if (a.x >= a.z) { return 0; }
  return 2;
}

vec2 domProject(vec3 p, int axis) {
  if (axis == 1) { return p.xz; }
  if (axis == 0) { return vec2(p.z, p.y); }
  return vec2(p.x, p.y);
}

// Inverse of domProject for in-plane (depth-free) vectors, e.g. a UV-space gradient back to world.
vec3 domUnproject(vec2 v, int axis) {
  if (axis == 1) { return vec3(v.x, 0., v.y); }
  if (axis == 0) { return vec3(0., v.y, v.x); }
  return vec3(v.x, v.y, 0.);
}

// Signed, centered cell-local coords on a square lattice of the given pitch.
vec2 gridCellLocal(vec2 uv, float pitch) {
  return (fract(uv / pitch) - 0.5) * pitch;
}

// Per-fragment frame the engine hands every grid-tier slot. cellLocal/cellId come from the
// lattice decomposition (engine-owned, cached across the march); t = curTimeSeconds.
struct GridCtx {
  vec2 cellLocal;
  vec2 cellId;
  float t;
};

// smoothstep paired with its derivative d/dx, so a profile's height and normal share one
// definition instead of hand-transcribing the 6t(1-t) slope. .x = value, .y = slope.
vec2 smoothstepVS(float e0, float e1, float x) {
  float t = clamp((x - e0) / (e1 - e0), 0., 1.);
  return vec2(t * t * (3. - 2. * t), 6. * t * (1. - t) / (e1 - e0));
}
