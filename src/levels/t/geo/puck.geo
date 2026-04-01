p = trace_path(
  || {
    circle(center=v2(0), radius=0.5)
  },
  center=true,
  reverse=false
)
p = path_scale(4, p)

spine_bevel_exponent = 5

export mesh = rail_sweep(
  spine_resolution=8,
  ring_resolution=32,
  spine=|u| v3(0, u * 1, 0),
  dynamic_profile=|u| {
    t = u * 2 - 1
    base = 1 - pow(abs(t), spine_bevel_exponent)
    delta = -0.5 + 1 * pow(max(base, 0), 1/spine_bevel_exponent)
    offset_path(p, delta, join_type='superellipse', superellipse_exponent=3)
  },
  spine_sampling_scheme={type: 'superellipse', bevel_fraction: 0.1, density: 3}
)
  | origin_to_geometry
