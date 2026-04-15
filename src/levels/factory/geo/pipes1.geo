path = catmull_rom_3d(
  [v3(-10, -150, 0), v3(30, -150, 0), v3(30, 0, 0), v3(30, 5, 0), v3(10, 5, 0), v3(10, 5, 15), v3(10, -35, 15), v3(-35, -35, 15)],
  tension=0.03
)

pipe = |radius: num, offset: vec3 = v3(0), scale: num = 1| {
  pt_cnt = 140
  points = 0..pt_cnt
    -> |i| {
      t = i/pt_cnt
      path(t) * scale
    }

  extrude_pipe(radius=radius, resolution=12, path=points)
    | simplify(tolerance=0.05)
    | trans(offset)
}

export mesh = (
  pipe(0.5, v3(2, 0, -1.2))
    + pipe(0.5, v3(0.95, -1, -0.8))
    + pipe(1, v3(-1, -1, -2.5), 1.08)
)
  | origin_to_geometry
