path = catmull_rom_3d(
  [v3(3, -10, 0), v3(-3, -10, 0), v3(-3, 20, 0), v3(3, 20, 0)],
  tension=0.03
)

pipe = |radius: num, offset: vec3 = v3(0), scale: vec3 = v3(1)| {
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
  pipe(1.5)
    + pipe(1.3, v3(0, 0, 3.7))
)
  | origin_to_geometry
