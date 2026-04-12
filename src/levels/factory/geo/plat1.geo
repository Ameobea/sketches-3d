p = trace_path(|| {
  circle(radius=35, center=v2(0))
})
p = path_intersect(
  p,
  trace_path(|| {
    move(-2, -6)
    line(2, -6)
    line(2, 6)
    line(-2, 6)
    close()
  }
)
  | path_scale(4, 3)
  | path_trans(0, -35))
  | path_trans(0, 25)

export mesh = p
  | tessellate_path
  | extrude(v3(0, 2, 0))
  | scale(0.5)
