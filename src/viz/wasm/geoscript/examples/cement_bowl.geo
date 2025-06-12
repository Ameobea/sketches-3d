spiral = |point_count, height_factor, radius, resolution| {
  0..point_count
    -> |i| vec3(
            sin(i * (1. / resolution)) * (radius + i/190),
            i * height_factor + sin(i/2)*1.4,
            cos(i * (1. / resolution)) * (radius + i/190)
           )
}

basket = spiral(point_count=4000, height_factor=0.01, radius = 9.5, resolution=16)
  | extrude_pipe(radius=0.5, resolution=8, close_ends=true)

basket + ((basket * 1.19) + vec3(0, -8, 0)) | render