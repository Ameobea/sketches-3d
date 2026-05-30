export mesh = cyl(radius=10, height=320, radial_segments=32, height_segments=16)
  -> |v, n| v + (n * v3(1, 0, 1)) * fbm(v* 0.02) * 0.8
