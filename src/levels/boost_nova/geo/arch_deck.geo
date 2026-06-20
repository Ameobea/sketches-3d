// Arch deck: a wide, thin slab swept along a semicircular arch. rail_sweep emits
// analytic UVs (U = arc length along the arch, V = around the slab cross-section), so a
// UV-keyed surface material can lay grate ticks that follow the curve instead of a
// world-axis projection.
profile = trace_svg_path('M -0.2 -2 L 0.2 -2 L 0.2 2 L -0.2 2 Z', center=true)

export mesh = rail_sweep(
  spine_resolution=96,
  ring_resolution=48,
  spine=|u| v3(cos(pi * u) * 12, sin(pi * u) * 12, 0),
  profile=profile,
  frame_mode=v3(0, 0, 1),
  capped=true
)
