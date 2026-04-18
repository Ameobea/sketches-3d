build_path = |offset: num| {
  p = trace_path(|| {
    move(-0.2, -100)
    line(0.2, -100)
    line(0.2 - offset, 100)
    line(-0.2 - offset, 100)
    close()
  })

  0..100
    -> |i| p | path_trans(i*1.8, 0)
    | fold(p, |acc, p| path_union(acc, p))
}

p = build_path(80)

p = path_union(p, build_path(-80) | path_trans(-100, 0))

p = path_intersect(p, trace_path(|| {
  move(-15.25, -24.7)
  line(15.25, -24.7)
  line(15.25, 24.7)
  line(-15.25, 24.7)
  close()
}) | path_trans(0.4, 0.3))

export mesh = p
  | tessellate_path(engine='lyon')
  | extrude(up=v3(0, 0.35, 0))
