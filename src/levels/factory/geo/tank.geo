export mesh = capsule(radius=14, height=8, radial_segments=28)
  | scale(1.15, 3.5, 1.15)
  | trans(0, -8, 0)
  | sub(b=box(100, 40, 100) + v3(0, 28, 0))
  -> |v| v * v3(2, 1, 2)
  -> |v| {
    if v.y < -3 {
      v3(v.x, -3 + (v.y -20) * 0.15, v.z)
    } else {
      v
    }
  }
  | union(
    cyl(radius=16, radial_segments=28, height=320) + v3(0, 160, 0)
  )
  | sub(b=(
    cyl(radius=30, radial_segments=28, height=16)
      - cyl(radius=11, radial_segments=28, height=60)
  ) + v3(0, 45, 0))
  | remesh_planar_patches
