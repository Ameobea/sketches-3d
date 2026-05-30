export mesh = [
  v3(0), v3(0, -12, 0), v3(0, -12, 12)
]
  | fillet_path_3d(radius=6, resolution=8)
  | extrude_pipe(radius=4.5, resolution=16)
  | origin_to_geometry
