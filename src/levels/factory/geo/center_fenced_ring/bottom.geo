post = cyl(radius=1.2, height=12, radial_segments=16)
  | trans(0, -5.6, 0)

bottom = cyl(radius=8, height=0.6, radial_segments=32)
  | sub(b=box(5, 30, 16) + v3(-3, 0, 0))
  | sub(b=box(8, 8, 4) + v3(-8, 0, -4.5))
  | sub(b=box(8, 8, 4) + v3(-8, 0, 4.5))
  | (
    (cyl(radius=8.2, height=18, radial_segments=32)
      - cyl(radius=7.5, height=22, radial_segments=32))
      | trans(0, 3, 0)
  )
  | trans(0, -11.4, 0)
  | rot(0, pi/2, 0)

export mesh = bottom | post
