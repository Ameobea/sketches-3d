// Corrugated-pipe demo for the corrugated_metal UV mode: rail_sweep's analytic UVs
// (U = spine arc length, V = around the ring) drive the corrugation, so the grooves
// wrap the rings (PAT_AXIS=0 in the material's constants). Profile radius 1.2 →
// circumference ≈ 7.54; the material's PAT_UV_SCALE.y of 7.7 = 14 × 0.55 pitch keeps
// the pattern wrap-seamless.
export mesh = rail_sweep(
  spine_resolution=140,
  ring_resolution=48,
  spine=|u| v3(u * 36 - 18, sin(u * 2 * pi) * 1.5, 0),
  profile=|u, v| v2(cos(v * 2 * pi), sin(v * 2 * pi)) * 1.2,
  frame_mode='rmf',
  capped=true,
  split_seams=true
)
