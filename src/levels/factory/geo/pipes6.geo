export mesh = [
  v3(0, 80, 0), v3(0, 0, 0), v3(270, 0, 0), v3(270, 80, 0)
]
  | fillet_path_3d(radius=6, resolution=8)
  | extrude_pipe(radius=4, resolution=16)
  | origin_to_geometry
