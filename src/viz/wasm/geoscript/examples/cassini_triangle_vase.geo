// Creates a vase-like shape using a stack of 3-foci Cassini curves.
// The foci are arranged in an equilateral triangle, giving the cross-section a rounded, star-like shape.

num_contours = 50
points_per_contour = 50
max_height = 20

contours = 0..num_contours -> |i| {
  t_height = i / num_contours
  y = t_height * max_height

  a = 1.5 + sin(t_height * pi * 4 - 0.9) * 0.5

  // `k` is the constant product of distances. To ensure a single connected curve,
  // we need k^3 to be greater than a^3. We'll vary it to change the 'pointiness' of the star.
  k_factor = 2.5 + (pow(cos(t_height * pi * 2 - 0.9)*0.5+0.5, 3.5)*2-1) * 1.48
  k = a * k_factor

  a_cubed = pow(a, 3)
  a_sixth = pow(a, 6)
  k_sixth = pow(k, 6)

  0..points_per_contour -> |j| {
    theta = (j / points_per_contour) * pi * 2

    // r^3 = a^3*cos(3*t) + sqrt(k^6 - a^6*sin^2(3*t))
    cos_3_theta = cos(3 * theta)
    sin_3_theta = sin(3 * theta)

    sqrt_term = max(k_sixth - a_sixth * pow(sin_3_theta, 2), 0)

    r_cubed = a_cubed * cos_3_theta + sqrt(sqrt_term)

    // We need the cube root. Since r_cubed can be negative, we handle it carefully.
    r = if r_cubed >= 0 {
      pow(r_cubed, 1/4)
    } else {
      // For negative values, take the cube root of the absolute value and flip the sign.
      -pow(-r_cubed, 1/3)
    }

    vec3(cos(theta) * r, y, sin(theta) * r)
  }
}

contours
  | stitch_contours(closed=true, cap_ends=true)
  | render
