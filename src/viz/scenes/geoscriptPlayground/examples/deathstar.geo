ring = 0..61
  -> (|i| vec3(sin(i*(pi/30)) * 10, 0, cos(i*(pi/30)) * 10))
  | extrude_pipe(radius=1.2, resolution=5, close_ends=true)
ball = icosphere(10, 4)
  - ring
  - (ring | rot(pi/2, 0, 0))
  | simplify(tolerance=0.01)
ball = ball
  - (box(0.81, 1.7, 2) | trans(-10, 0, 0))
  - (box(0.81, 1.7, 2) | trans(10, 0, 0))
  | simplify(tolerance=0.01)

obj = box(5, 13, 15)
  | rot(-0.4, 0.2, -0.9)
  | trans(-7, -5, 4)
  | tess(target_edge_length=0.1)
  | warp(|v, norm| v + norm * (fbm(v*0.25) * 0.5 + 0.5) * 3)

nicks = ball
  | point_distribute(count=40)
  -> (|p| {
        box(randf(1.2, 2.5), randf(1.2, 9.5), randf(1.2, 2.5))
          | rot(randf(), randf(), randf())
          | trans(p)
          | tess(target_edge_length=0.2)
          | warp(|v, norm| v + norm * (fbm(v*0.6) * 0.5 + 0.5))
      })
  | join

(ball - nicks - obj)
  | simplify(tolerance=0.05)
  | render
