p = trace_svg_path('M0 3 0 0 2-3 4-2M-.5 3 .5 3M4.2236-2.4472 3.7764-1.5528')
p = offset_path(p, delta=0.4, end_type='square', join_type='miter')
p = offset_path(p, delta=-0.3, end_type='superellipse', join_type='superellipse', superellipse_exponent=2)
p = offset_path(p, delta=0.25, end_type='superellipse', join_type='superellipse', superellipse_exponent=8)


// 0..10000
//   -> |i| {
//     v = p(i/10000)
//     vec3(v.x, 0, v.y)
//   }
//   | render

spine_bevel_exponent = 8

rail_sweep(
  spine_resolution=7,
  ring_resolution=96,
  spine=|u| v3(0, 0, u * 0.4),
  dynamic_profile=|u| {
    t = u * 2 - 1
    base = 1 - pow(abs(t), spine_bevel_exponent)
    delta = -0.1 + 0.1 * pow(max(base, 0), 1/spine_bevel_exponent)

    offset_path(p, delta, join_type='superellipse', superellipse_exponent=2)
  },
  spine_sampling_scheme=[0.012, 0.03, 0.08, 0.5, 0.92, 0.97, 0.988]
)
  // | remesh_planar_patches(max_angle_deg=2)
  | simplify(tolerance=0.001)
  | render
