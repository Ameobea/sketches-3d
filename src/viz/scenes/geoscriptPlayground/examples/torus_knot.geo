torus_knot_path(radius=4.5, tube_radius=2, p=3, q=11, point_count=420)
  | extrude_pipe(radius=0.3, resolution=12, close_ends=true)
  | simplify(tolerance=0.01)
  | render
