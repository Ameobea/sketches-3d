ring = |radius, width, height|: mesh
  box(radius, height, radius) - box(radius-width, 20, radius-width)

rings = 1..8 -> (|i| ring(i*5, width=2, height=2)) | join

all: mesh = rings
  + (rings | rot(pi/2,0,0))
  + (rings | rot(0,0,pi/2))

all = all - box(12)

all = (all | rot(0,pi,0))
  | tess(target_edge_length=0.2)
  | warp(|v| {
    dist_to_origin: float = len(v * vec3(1, 0.5, 1))
    v + vec3(0, sin(v.x/2+3) * sin(v.z/2+3), 0) - vec3(0, dist_to_origin * 0.9, 0)
  })

all + vec3(0,-10,0) | render

all | rot(pi, 0, 0) | trans(vec3(0,10,0)) | render

box(4) | render