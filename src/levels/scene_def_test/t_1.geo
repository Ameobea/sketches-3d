p = trace_svg_path(
  'M14.21 7.01Q14.11 7.92 14.08 8.77Q14.04 9.62 14.04 10.08Q14.04 10.66 14.06 11.16Q14.09 11.66 14.11 12.05L13.56 12.05Q13.39 10.37 13.1 9.4Q12.82 8.42 12.1 7.99Q11.38 7.56 9.86 7.56L8.57 7.56L8.57 21.22Q8.57 22.18 8.75 22.66Q8.93 23.14 9.46 23.3Q9.98 23.47 10.99 23.52L10.99 24Q10.37 23.95 9.41 23.94Q8.45 23.93 7.46 23.93Q6.38 23.93 5.44 23.94Q4.49 23.95 3.91 24L3.91 23.52Q4.92 23.47 5.45 23.3Q5.98 23.14 6.16 22.66Q6.34 22.18 6.34 21.22L6.34 7.56L5.04 7.56Q3.55 7.56 2.82 7.99Q2.09 8.42 1.8 9.4Q1.51 10.37 1.34 12.05L0.79 12.05Q0.84 11.66 0.85 11.16Q0.86 10.66 0.86 10.08Q0.86 9.62 0.83 8.77Q0.79 7.92 0.7 7.01Q1.7 7.03 2.89 7.06Q4.08 7.08 5.28 7.08Q6.48 7.08 7.46 7.08Q8.45 7.08 9.64 7.08Q10.82 7.08 12.02 7.06Q13.22 7.03 14.21 7.01Z',
  center=true
)
p = offset_path(p, delta=0.9, end_type='square', join_type='miter')
p = offset_path(p, delta=-0.7, end_type='superellipse', join_type='superellipse', superellipse_exponent=2)
p = offset_path(p, delta=0.7, end_type='superellipse', join_type='superellipse', superellipse_exponent=2)


// // 0..10000
// //   -> |i| {
// //     v = p(i/10000)
// //     vec3(v.x, 0, v.y)
// //   }
// //   | render

spine_bevel_exponent = 8

m = rail_sweep(
  spine_resolution=70,
  ring_resolution=96,
  spine=|u| v3(0, sin(u * 3.3) * 20, u * 80),
  dynamic_profile=|u| {
    t = u * 2 - 1
    base = 1 - pow(abs(t), spine_bevel_exponent)
    delta = -0.1 + 0.1 * pow(max(base, 0), 1/spine_bevel_exponent)

    offset_path(p, delta, join_type='superellipse', superellipse_exponent=2)
  },
  twist=|i| i * 0.054,
  // spine_sampling_scheme=[0.012, 0.03, 0.08, 0.5, 0.92, 0.97, 0.988]
)
  // | remesh_planar_patches(max_angle_deg=2)
  // | simplify(tolerance=0.01)
  | rot(0, pi/2, 0)
  // | render

export mesh = m

// text_to_mesh(' T', font_family='Playfair Display', depth=2) | render
