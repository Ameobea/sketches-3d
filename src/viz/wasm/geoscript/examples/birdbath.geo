cyl(radius=4, height=10, radial_segments=20, height_segments=5)
  -> |v| {
    dist = abs(v.y)
    displ = 0.23 + pow(dist * 0.2 + 0.1, 3.)
    v * v3(displ, 1, displ)
  }
  | sub(b=icosphere(radius=5.7, resolution=2) + vec3(0, 10.3, 0) | scale(vec3(1, 1.4, 1)))
  | intersect(cyl(radius=5.3, height=20, radial_segments=20, height_segments=20))
  -> |v| {
    y = max(v.y, -3.8)
    shrink = if y < 0 { 1 + y * 0.08 } else { 1 }
    v * v3(shrink, 1, shrink)
  }
  | render
