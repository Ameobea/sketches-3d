radius = 16

blocker = cyl(radius=radius+4, height=2, radial_segments=32)
  | rot(pi/2, 0, 0)
  | sub(b=box(100, 20, 100) - v3(0, 10, 0))
  | sub(b=box(8, 100, 8) + v3(radius + 4, 0, 0))
  | sub(b=box(8, 100, 8) - v3(radius + 4, 0, 0))
  | trans(0, 0.1, 0)

plat = cyl(radius=radius, height=1, radial_segments=32)
  - (cyl(radius=2.6, height=10, radial_segments=32) + v3(0, 0, 6.5))
  - (cyl(radius=2.6, height=10, radial_segments=32) + v3(0, 0, -9.5))
  | blocker

export mesh = plat
