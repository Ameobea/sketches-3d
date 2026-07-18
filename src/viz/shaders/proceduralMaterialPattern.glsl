// Dual-mode pattern-space plumbing for procedural materials: canonical,
// constants-overridable knobs select between world-space dominant-axis
// projection and mesh-UV pattern space (rail_sweep UVs: U = spine arc length in
// world units, V = ring param wrapping [0,1)). A variant flips a material to UV
// mode via `shaders.constants` — no per-material defines needed. Emitted after
// the constant defines and before the common slot, so overrides apply here.
//   PAT_UV_MODE  0 = world-space dominant-axis projection, 1 = mesh-UV space
//   PAT_UV_SCALE pattern units per vUv unit (UV mode)
//   PAT_OFFSET   pattern-space alignment shift
//   PAT_AXIS     1 = swap pattern axes (e.g. pipe grooves: rings ↔ spans)
#ifndef PAT_UV_MODE
#define PAT_UV_MODE 0
#endif
#ifndef PAT_UV_SCALE
#define PAT_UV_SCALE vec2(1.0)
#endif
#ifndef PAT_OFFSET
#define PAT_OFFSET vec2(0.0)
#endif
#ifndef PAT_AXIS
#define PAT_AXIS 0
#endif

// Surface position → pattern space.
vec2 patProjectUV(vec3 pos, vec3 axisNormal) {
#if PAT_UV_MODE == 1
  vec2 p = vUv * PAT_UV_SCALE - PAT_OFFSET;
#else
  vec2 p = domProject(pos, domAxis(axisNormal)) - PAT_OFFSET;
#endif
#if PAT_AXIS == 1
  p = p.yx;
#endif
  return p;
}

// Per-axis pixel footprint in pattern units (axis-swapped to match patProjectUV).
vec2 patAA() {
#if PAT_UV_MODE == 1
  vec2 aa = aaUvFootprint * PAT_UV_SCALE;
#if PAT_AXIS == 1
  aa = aa.yx;
#endif
  return aa;
#else
  return vec2(aaWorldFootprint);
#endif
}

// Pattern-space carve gradient (∂u, ∂v) → world, for relief normals. UV mode
// maps through the mesh tangent frame — the dominant-axis mapping switches
// basis mid-surface on curved sweeps, flipping the relief along a visible line.
vec3 patGradToWorld(vec2 grad, vec3 N) {
#if PAT_AXIS == 1
  grad = grad.yx;
#endif
#if PAT_UV_MODE == 1
  return grad.x * uvFrameT + grad.y * uvFrameB;
#else
  return domUnproject(grad, domAxis(N));
#endif
}
