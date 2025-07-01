// bumpy sphere that kinda looks like an artichoke

num_contours = 70
points_per_contour = 150
radius = 10
bump_frequency = 8
bump_amplitude = 2

contours = 0..num_contours -> |i| {
  u = i / (num_contours - 1)
  theta = u * pi
  y = cos(theta) * radius
  contour_radius = sin(theta) * radius

  0..points_per_contour -> |j| {
    v = j / points_per_contour

    bumpy_radius = contour_radius + sin(v * pi * 2 * bump_frequency) * sin(theta * bump_frequency) * bump_amplitude

    vec3(cos(v * pi * 2) * bumpy_radius, y, sin(v * pi * 2) * bumpy_radius)
  }
}

contours
  | stitch_contours(closed=true, cap_ends=true, flipped=true)
  | simplify(tolerance=0.05)
  | render
