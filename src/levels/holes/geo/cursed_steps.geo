p = trace_path(|| {
  move(0, 0)
  line(20, 0)

  move(0, 0)
  line(0, 10)

  move(4, 0)
  line(4, 8)

  move(8, 0)
  line(8, 6)

  move(12, 0)
  line(12, 4)

  move(16, 0)
  line(16, 2)
})
  | path_scale(1.3, 1.5)
  | offset_path(delta=0.8, end_type='square')

export mesh = p
  | tessellate_path
  | extrude(up=v3(0, 3, 0))
  | rot(-pi/2, 0, 0)
  | apply_transforms
  | origin_to_geometry