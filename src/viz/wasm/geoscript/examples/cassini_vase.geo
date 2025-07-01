// a pleasing vase-like shape created using a stack of Cassini ovals

num_contours = 80
points_per_contour = 100
max_height = 20

contours = 0..num_contours -> |i| {
  t = i / num_contours
  y = t * max_height

  a = 2 + sin(t * pi * 2)

  c_squared = pow(a * 1.5, 2)
  c_fourth = pow(c_squared, 2)

  // generating the contour using the polar equation for a Cassini oval
  0..points_per_contour -> |j| {
    theta = (j / points_per_contour) * pi * 2

    // r^2 = a^2*cos(2*t) + sqrt(c^4 - a^4*sin^2(2*t))
    // using the positive root to get the outer curve
    a_squared = pow(a, 2)
    a_fourth = pow(a, 4)
    cos_2_theta = cos(2 * theta)
    sin_2_theta = sin(2 * theta)

    // This is the formula for r^2 derived from the polar equation
    r_squared = a_squared * cos_2_theta + sqrt(c_fourth - a_fourth * pow(sin_2_theta, 2))

    // Ensure r_squared is not negative due to floating point errors
    r_squared = max(r_squared, 0)
    r = sqrt(r_squared)

    vec3(cos(theta) * r, y, sin(theta) * r)
  }
}

contours
  | stitch_contours(closed=true, cap_ends=true)
  | render
