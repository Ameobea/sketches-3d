m = capsule(radius=14, height=8, radial_segments=28)
  | scale(1.15, 3.5, 1.15)
  | trans(0, -8, 0)
  | sub(b=box(100, 40, 100) + v3(0, 24, 0))
  | scale(2, 1, 2)
  -> |v: vec3| {
    if v.y < -3 {
      v3(v.x, -3 + (v.y -20) * 0.15, v.z)
    } else {
      v
    }
  }
  -> |v: vec3| {
    hs = 0.935 + (v.y + 3) * 0.008
    v3(v.x * hs, v.y, v.z * hs)
  }
  | union(
    cyl(radius=16, radial_segments=28, height=320) + v3(0, 160, 0)
  )
  | sub(b=(
    cyl(radius=30, radial_segments=28, height=16)
      - cyl(radius=11, radial_segments=28, height=60)
  ) + v3(0, 45, 0))

p = trace_path(|| {
  move(-8, 0)
  cubic_bezier(v2(-7.5, 8.5), v2(-7.5, 15.5), v2(-4, 16))
  // quadratic_bezier(v2(-2.3, 20), v2(0, 20.2))
  line(0, 16)
  line(0,0)
  close()
}, reverse=true)
  | path_scale(3.45, 5)

blocker = rail_sweep(
  spine_resolution=4,
  ring_resolution=20,
  spine=|u| {
    base = v3(6, 0, 6)
    theta = u * tau * 0.07
    v3(base.x * cos(theta), base.z, base.x * sin(theta))
  },
  profile=p,
  closed=false,
  capped=true
)
  | rot(pi, 0, 0)
  | trans_global(0, -8, 0)

export mesh = m
  | (blocker | rot_global(0, (1*tau)/3, 0))
  | (blocker | rot_global(0, (2*tau)/3, 0))
  | (blocker | rot_global(0, (3*tau)/3, 0))
  | remesh_planar_patches
