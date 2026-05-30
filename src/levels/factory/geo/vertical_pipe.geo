export mesh = [
  v3(0), v3(0, 50, 0), v3(0, 50, 20)
]
  | fillet_path_3d(radius=6, resolution=8)
  | extrude_pipe(radius=4.5, resolution=16)
  | origin_to_geometry
