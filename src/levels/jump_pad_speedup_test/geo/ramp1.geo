export mesh = 0..20
  -> |i| {
    t = i/20
    y = pow(t, 3)
    v3(t * 5, mix(0.3, y, t) * 3, 0)
  }
  | extrude_path(up=v3(0, 0, 5))
  // | extrude_along_normals(0.2)
  | extrude(up=v3(0, -0.2, 0))
