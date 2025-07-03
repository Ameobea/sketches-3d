ring = cyl(radius=6, height=1.5, height_segments=1, radial_segments=64)
  - (cyl(radius=2.5, height=1.5, height_segments=1, radial_segments=64)
      -> |v| v * if v.y > 0 {v3(0.8,1,0.8)} else {v3(1.2,1,1.2)})

structure = 0..41
  -> |i| {
    radius = pow((20 - abs(20 - i)) * 0.3, 1.56)
    superellipse_path(width=radius, height=radius, n=0.5+0.03*radius, point_count=60)
      -> |v| v3(v.x, i*0.4, v.y)
  }
  | stitch_contours(cap_ends=true)
  | scale(1, 1 + 1/3, 1)
  | simplify(tolerance=0.02)
  | sub(b=ring | rot(pi,0,0) | trans(0,8,0))
  | sub(b=ring + v3(0,13.5,0))
  | simplify(tolerance=0.02)
  | render
