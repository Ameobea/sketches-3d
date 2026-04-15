path = catmull_rom_3d(
  [v3(-150, 100, 0), v3(-150, 0, 0), v3(0), v3(0, 0, 18), v3(150, 0, 18), v3(150, 100, 18)],
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

export mesh = pipe(5)
  | origin_to_geometry
