export mesh = trace_svg_path('m 77.5,92.5 h -45 a 30,30 0 0 1 0,-60 h 20 v -15 l 30,23 -30,23 v -15 h -20 a 14,14 0 0 0 0,28 h 45 z', center=true)
  | tessellate_path
  | extrude(up=v3(0, 4, 0))
  | scale(0.2)
