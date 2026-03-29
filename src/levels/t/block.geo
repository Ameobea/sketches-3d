p = trace_path(
  || {
    move(-1, 0)
    line(1, 0)
  },
  center=true,
  reverse=false
)
p = offset_path(p, delta=1, end_type='superellipse', superellipse_exponent=5)

spine_bevel_exponent = 5

export mesh = rail_sweep(
  spine_resolution=2,
  ring_resolution=32,
  spine=|u| v3(0, u * 4, 0),
  profile=p,
  spine_sampling_scheme={type: 'superellipse', bevel_fraction: 0.1, density: 3}
)
  | origin_to_geometry
  | remesh_planar_patches(max_angle_deg=2)
