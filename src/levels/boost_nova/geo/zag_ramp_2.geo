p2 = build_path(path {
  move(36, 20)
  line(28, 25)
  line(26, 18)
  line(6, 34)
}, closed=false)

export mesh = extrude_path(p2, up=v3(0, 1, 0))
  | extrude(up=v3(0.2, 0, -0.5))
  | rot(-pi/2, 0, 0)
