outer = 0..1000
  -> |i| { vec3(0, 0, i/10) }
  | extrude_pipe(
      radius=|i: int|: float 3 + ((sin(i * 0.02) + 1) / 2) * 5,
      resolution=4,
      twist=|i: int, pos: vec3|: float pi/4 + i * 0.03
    )

inner = 0..102
  -> |i| { vec3(0, 0, i) }
  | extrude_pipe(radius=|i| 1.5 + sin(i*0.5) * 0.4, resolution=18)

outer - inner | render
