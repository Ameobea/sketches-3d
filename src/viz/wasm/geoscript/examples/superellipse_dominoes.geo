tile = |n: num| {
  path = superellipse(width=4, height=10, n=n, point_count=50) -> |v| vec3(v.x, 0, v.y)

  blocker = path
    | fan_fill(flipped=true)
    | extrude(up=vec3(0,4,0))
    | trans(0,0,-2)
    | rot(pi/2,0,0)

  box(6,15,0.5) - blocker
}

0..10 -> |i| {
  n = 0.5 + i * 0.2
  tile(n) + vec3(0,0,i*4)
} | render
