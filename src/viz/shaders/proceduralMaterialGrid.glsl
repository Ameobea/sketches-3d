// Shared substrate for capability-ladder POM materials (see pom-capability-ladder-plan.md).
// The dominant-axis projection helpers (domAxis/domProject/domUnproject) and smoothstepVS
// live in proceduralMaterialAA.glsl (always emitted first); this file adds the cell
// decomposition the marcher hoists out of its inner loop.

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
