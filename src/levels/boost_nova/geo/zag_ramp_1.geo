p1 = build_path(path {
  line(8, 5)
  line(10, 0)
  line(20, 6)
  line(21, 1)
  line(36, 10)
}, closed=false)

export mesh = extrude_path(p1, up=v3(0, 1, 0))
  | extrude(up=v3(0.2, 0, 0.5))
  | rot(-pi/2, 0, 0)
