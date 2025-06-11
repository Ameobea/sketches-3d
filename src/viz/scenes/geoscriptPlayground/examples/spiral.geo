spiral = |point_count, height_factor, radius, resolution| {
  0..point_count
    -> |i| { vec3(sin(i * (1. / resolution)) * radius, i * height_factor, cos(i * (1. / resolution)) * radius) }
}

spiral(point_count=200, height_factor=0.2, radius = 5.5, resolution=2)
  | extrude_pipe(radius=0.5, resolution=8, close_ends=true)
  | render
