pipe = |radius: num, offset: vec3 = v3(0)| {
  points = [v3(-50, 0, 0), v3(50, 0, 0)]

  extrude_pipe(radius=radius, resolution=12, path=points)
    | simplify(tolerance=0.05)
    | trans(offset)
}

export mesh = (
  pipe(3)
    + pipe(3, v3(0, 0.4, 8.5))
    + pipe(1, v3(0, 0, 4.4))
    + pipe(0.6, v3(0, -1.5, 5.2))
    + pipe(0.6, v3(0, -1.5, 3.2))
)
  | origin_to_geometry
