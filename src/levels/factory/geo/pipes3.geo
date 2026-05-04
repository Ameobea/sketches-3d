export mesh = ([v3(0, -10, 2), v3(0, -10, -2), v3(0, 0, -2), v3(3, 0, -2), v3(3, 10, -2), v3(3, 10, 2)]
  | fillet_path_3d(radius=0.4, resolution=4)
  | extrude_pipe(radius=0.47, resolution=8))
  + (
    [v3(1, -10, 2), v3(1, -10, -2), v3(1, -0.8, -2), v3(4, -0.8, -2), v3(4, 10, -2), v3(4, 10, 2)]
      | fillet_path_3d(radius=0.4, resolution=4)
      | extrude_pipe(radius=0.47, resolution=8)
  )
  + (
    [v3(2, -10, 2), v3(2, -10, -2), v3(2, -2, -2), v3(2, -2, -3), v3(2, 1, -3), v3(2, 1, -2), v3(2, 10, -2), v3(2, 10, 2)]
      | fillet_path_3d(radius=0.4, resolution=4)
      | extrude_pipe(radius=0.47, resolution=8)
  )
