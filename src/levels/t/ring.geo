p = trace_svg_path(
  'M5.23 23.62Q5.04 21.96 4.49 20.56Q3.94 19.15 2.9 17.71Q5.3 17.81 6.31 17.81Q11.21 17.81 14.33 16.92Q15.36 16.63 15.66 16.39Q15.96 16.15 15.96 15.65Q15.96 14.76 15.04 14.29Q14.11 13.82 12.36 13.82Q10.46 13.82 7.51 14.33Q4.56 14.83 2.59 15.5L1.46 10.18Q4.78 9.48 7.74 9.08Q10.7 8.69 12.79 8.69Q15.72 8.69 17.82 9.46Q19.92 10.22 21.04 11.69Q22.15 13.15 22.15 15.17Q22.15 18.53 19.91 20.36Q17.66 22.2 14.02 22.87Q10.37 23.54 5.23 23.62Z',
  center=true
)
p = path_difference(p, trace_path(|| {
  move(-5, -5)
  line(-5, 5)
  line(4.25, 5)
  line(4.25, -5)
  close()
}))
p = offset_path(p, delta=0.5, join_type='round')

// 0..10000
//   -> |i| {
//     v = p(i/10000)
//     vec3(v.x, 0, v.y)
//   }
//   | render

spine_bevel_exponent = 2
ring_resolution = 56
spine_resolution = 100

spine_resolution=50

export mesh = rail_sweep(
  spine_resolution=spine_resolution,
  ring_resolution=ring_resolution,
  spine=|u| v3(cos(u * 0.98 * pi * 2) * 80, 0, sin(u * 0.99 * pi * 2) * 80),
  profile=p,
  closed=true
)
  | remesh_planar_patches(max_angle_deg=4)
  | rot(0, pi/2, 0)
  | scale(1, 2, 1)
