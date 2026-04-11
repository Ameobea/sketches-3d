m = sphere(radius=10, resolution=2)
  | scale(1, 1, 1.25)
  | sub(b=box(50, 50, 10) + v3(0, 0, 8.5))
  | sub(b=box(50, 50, 10) - v3(0, 0, 8.5))
  | remesh_planar_patches

b = 0..5
  -> |i| {
    box(20, 4.5, 12)
      | trans(14, 0, 0)
      | rot_global(0, 0, i * (tau / 5))
  }
  | union

m = ((m * v3(1.2, 1.2, 1)) & b) | (m * v3(0.55, 0.55, 1))
  // | simplify(tolerance=0.05)w
  -> |v| v3(v.x, v.y, round(v.z * 10) / 10)
  | remesh_planar_patches
  -> |v| v3(v.x, v.y, round(v.z * 10) / 10)

rod = cyl(radius=1.2, height=65, radial_segments=5, height_segments=4)
  | rot(0, 0, pi/2)
  | rot_global(0, pi/2, -0.2)
  | trans_global(0, 0, 30)

export mesh = m | rod
