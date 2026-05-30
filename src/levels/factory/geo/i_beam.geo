set_sharp_angle_threshold(0)

p = build_path(path {
  rect(v2(0, -1.25), v2(2.4, 0.35))
  rect(v2(0, 1.25), v2(2.4, 0.35))
  rect(v2(0), v2(0.3, 2.5))
}, center=true)
p = path_union(p, p)
  | offset_path(join_type='bevel', delta=0.18)
  | offset_path(join_type='miter', delta=-0.18)

export mesh = tessellate_path(p)
  | extrude(up=v3(0, 20, 0))
  | origin_to_geometry
  | scale(2.5, 2.4, 3)
  | rot(pi/2, 0, 0)
  | apply_transforms
