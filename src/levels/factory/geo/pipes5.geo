export mesh = [
  v3(0, 24, 8), v3(0, 24, 0), v3(0), v3(-18, 0, 0),
  v3(-18, 0, 2), v3(-18, 0, 16), v3(-18, -24, 16),
  v3(-10, -24, 16)
]
  | fillet_path_3d(radius=3.5, resolution=4)
  | extrude_pipe(radius=2, resolution=12)
  | origin_to_geometry
