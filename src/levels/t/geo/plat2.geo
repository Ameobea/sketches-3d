p = trace_path(
  || {
    line(0,1)
    line(1,1)
    line(1,0)
    close()
  },
  center=true,
  reverse=true
)
p = path_scale(4, p)

spine_bevel_exponent = 5

m = rail_sweep(
  spine_resolution=12,
  ring_resolution=16,
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
  | remesh_planar_patches(max_angle_deg=2)

p = trace_path(|| {
  move(-3.6, 0)
  line(3.6, 0)
})
  | offset_path(delta=1.5, arc_tolerance=0.02)
  | path_scale(1/3)
  | path_rot(pi/4)

s = tessellate_path(p)
  | extrude(up=v3(0, 3, 0))
  | origin_to_geometry

// 0..10000
//   -> |i| {
//     v = p(i/10000)
//     vec3(v.x, 0, v.y)
//   }
//   | render

export mesh = m - (s | trans(-0.75, 0, 0.75)) - (s | trans(0.75, 0, -0.75))
