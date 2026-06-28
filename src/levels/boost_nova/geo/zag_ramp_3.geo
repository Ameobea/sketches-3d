p3 = build_path(path {
  move(4, 24)
  line(-5, 32)
}, closed=false)

export mesh = extrude_path(p3, up=v3(0, 1, 0))
  | extrude(up=v3(0.2, 0, -0.5))
  | rot(-pi/2, 0, 0)
