p = trace_svg_path(
  "M14.18 9.31L8.52 9.31L8.52 24L6.12 24L6.12 9.31L0.46 9.31L0.46 7.2L14.18 7.2Z",
  center=true
)

spine_bevel_exponent = 8

export mesh = rail_sweep(
  spine_resolution=32,
  ring_resolution=36,
  spine=|u| v3(0, sin(u * 3.3) * 20, u * 80),
  dynamic_profile=|u| {
    t = u * 2 - 1
    base = 1 - pow(abs(t), spine_bevel_exponent)
    delta = -1 + 2 * pow(max(base, 0), 1/spine_bevel_exponent)

    offset_path(p, delta, join_type='superellipse', superellipse_exponent=3)
  },
  spine_sampling_scheme={type: 'bevel', bevel_fraction: 0.05, density: 8}
)
  | remesh_planar_patches(max_angle_deg=2)
  | origin_to_geometry
