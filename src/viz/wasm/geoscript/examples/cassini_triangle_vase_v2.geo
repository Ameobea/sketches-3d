num_contours = 70
points_per_contour = 50
max_height = 20

contours = 0..num_contours -> |i| {
  t_height = i / num_contours
  y = t_height * max_height

  a = 1.5 + sin(t_height * pi) * 0.2 + (1-t_height) * 2

  // `k` is the constant product of distances. To ensure a single connected curve,
  // we need k^3 to be greater than a^3. We'll vary it to change the 'pointiness' of the star.
  k_factor = 1.5 + (pow(cos(t_height * pi * 4 + 3)*0.5+0.5, 2.5) * 2 - 1) * 0.45
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
      pow(r_cubed, 1/3)
    } else {
      // For negative values, take the cube root of the absolute value and flip the sign.
      -pow(-r_cubed, 1/3)
    }

    vec3(cos(theta) * r, y, sin(theta) * r)
  }
}

vase = contours
  | stitch_contours(closed=true, cap_ends=true)
  // elongate the base and the rim at the top
  -> |v| {
    heightFactor = max(smoothstep(16, 20, v.y), 1-smoothstep(0, 4, v.y)) * 2
    distFromCenter = min(distance(v2(v.x, v.z), v2(0)), 2.5)
    factor = max(heightFactor * (1-(distFromCenter/2.6)) * 2 + heightFactor * 0.3, -0.5)
    v * vec3(1+factor, 1, 1+factor)
  }
interior = vase
  | scale(0.96, 1.0001, 0.96)
  | trans(0,0.01,0);
vase = (vase - interior)
  | simplify(tolerance=0.09)

vase | render
// swap this /\ for this \/ to see something cool
// vase | trace_geodesic_path([v2(0,1000)]) | extrude_pipe(radius=0.3, resolution=8) | simplify(tolerance=0.09) | render
