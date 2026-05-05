export mesh = (
    [v3(-1, -10, 2), v3(-1, -10, -2), v3(-1, -6, -2), v3(2, -6, -2), v3(2, 10, -2), v3(2, 10, 2)]
      | fillet_path_3d(radius=0.8, resolution=4)
      | extrude_pipe(radius=0.7, resolution=8)
  )
  + (
    [v3(7, -10, 2), v3(7, -10, -2), v3(7, -3, -2), v3(4, -3, -2), v3(4, 10, -2), v3(4, 10, 2)]
      | fillet_path_3d(radius=0.8, resolution=4)
      | extrude_pipe(radius=0.7, resolution=8)
      | trans(-0.2, 0, 1)
  )
