torus_knot_path(radius=5, tube_radius=2, p=3, q=4, point_count=400)
  | extrude_pipe(radius=1.1, resolution=12, close_ends=true)
  | simplify(tolerance=0.05)
  | render
