p = circle(radius=35, center=v2(0))
p = path_intersect(
  p,
  path()
    | move(-2, -6)
    | line(2, -6)
    | line(2, 6)
    | line(-2, 6)
    | close
    | scale(4, 3)
    | translate(0, -35)
)
  | translate(0, 25)

export mesh = p
  | tessellate_path
  | extrude(v3(0, 2, 0))
  | remesh_planar_patches
  | scale(0.5)
