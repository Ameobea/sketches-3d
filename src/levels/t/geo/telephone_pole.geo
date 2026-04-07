p = trace_svg_path(
  "M14.18 9.31L8.52 9.31L8.52 24L6.12 24L6.12 9.31L0.46 9.31L0.46 7.2L14.18 7.2Z",
  center=true
) | path_scale(1.3, 1)
p = p | path_union(p | path_trans(offset=v2(0, 9)))
// p = p | path_union(p | path_rot(angle=-pi) | path_trans(0, -15))
p = path_difference(p, trace_path(|| {
  move(1, 1)
  line(-1, 1)
  line(-1, -1)
  line(1, -1)
  close()
}, center=true) | path_scale(8, 8) | path_trans(0, 16))

spine_bevel_exponent = 5

export mesh = rail_sweep(
  spine_resolution=14,
  ring_resolution=90,
  spine=|u| v3(u*5, 0, 0),
  dynamic_profile=|u| {
    t = u * 2 - 1
    base = 1 - pow(abs(t), spine_bevel_exponent)
    delta = 0.5 * pow(max(base, 0), 1/spine_bevel_exponent)

    offset_path(p, delta, join_type='superellipse', superellipse_exponent=5.2)
  },
  spine_sampling_scheme={type: 'bevel', bevel_fraction: 0.1, density: 3}
)
  | remesh_planar_patches(max_angle_deg=2)
  | origin_to_geometry
  // | scale(1, 0.8, 1.5)
