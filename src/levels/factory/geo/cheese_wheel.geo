spine_bevel_exponent = 8

width = 12
cheese_wheel = rail_sweep(
  spine_resolution=8,
  ring_resolution=50,
  spine=|i| v3(i*width, 0, 0),
  dynamic_profile=|u: float| {
    t = u * 2 - 1
    base = 1 - pow(abs(t), spine_bevel_exponent)
    delta = 2.5 + 1 * pow(max(base, 0), 1/spine_bevel_exponent)

    p = trace_path(|| circle(v2(0), 5))
    offset_path(path=p, delta=delta)
  },
  spine_sampling_scheme=[0.001, 0.01, 0.05, 0.1, 0.9, 0.95, 0.99, 0.999]
)
  | remesh_planar_patches

inner = trace_path(|| circle(v2(0), 5))
  | path_difference(clip=trace_path(|| circle(v2(0), 4)))
  | path_difference(clip=trace_path(|| {
    move(-0.8, -2)
    line(0.8, -2)
    line(0.8, 8)
    line(-0.8, 8)
    close()
  }))
  | path_scale(1.4)

critical_points(inner) | print

inner = inner
  | tessellate_path(curve_angle_degrees=2, sample_count=120)
  | extrude(up=v3(0, 1.4, 0))
  | rot_global(0, 0, pi/2)
  | rot_global(1.8, 0, 0)

export mesh = cheese_wheel - (
  box(90, 9, 9)
    | scale(1, 2, 1)
    | rot_global(0.6, 0, 0)
    | trans_global(0, 1.7, 6)
)
  | union(inner | trans_global(4.5, 0, 0))
  | union(inner | trans_global(9, 0, 0))
  | rot_global(pi/4 + 0.2, 0, 0)
  | sub(b=box(100, 8, 100) + v3(7.5))
  | remesh_planar_patches
