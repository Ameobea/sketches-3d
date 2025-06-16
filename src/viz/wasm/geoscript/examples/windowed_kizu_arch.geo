end = vec3(60, 0, 0)
depth = 30
center = vec3(end.x / 2, 0, 0)
path = |count: int|: seq {
  bezier3d(vec3(0), vec3(5, 0, depth), vec3(end.x - 5, 0, depth), end, count)
};

(path(20)
  | extrude_pipe(radius=3, resolution=12, close_ends=true)
  | scale(1, 1.5, 1))
  - (path(14)
      | skip(1)
      | take(13)
      -> |pos: vec3| {
        norm = normalize(pos - center)
        // TODO: These windows are crooked
        dir = look_at(pos, center - vec3(0, 4, 0))
        window = box(5, 3.2, 5) | rot(vec3(-0.2, -dir.y, 0));
        (window + (pos + vec3(0, 0.8, 0))) + norm
      }
      | join
    )
  | (path(28)
      | skip(1)
      | take(27)
      | filter(|pos, i| i % 2 == 0)
      -> |pos: vec3| {
        norm = normalize(pos - center)
        dir = look_at(pos, center - vec3(0, 4, 0))
        pillar = box(0.7, 7.5, 2)
          | tess(target_edge_length=1.6)
          | warp(|v| {
            if v.y > 0 {
              return v
            }
            v - vec3(0, 0, pow(-v.y * 0.2, 2.4)*4)
          })
          | rot(vec3(-0.1, -dir.y, 0))
        (pillar + (vec3(0, -0.1, 0) + pos)) + norm*2.5
      }
      | join
  )
  | (path(28)
      | skip(1)
      | take(27)
      -> |pos| {
        dir = look_at(pos, center)
        box(1, 14, 2.3)
          | rot(vec3(0, -dir.y, 0))
          | trans(pos - vec3(0, 9, 0))
      }
      | join
  )
  | (cyl(radius=10, height=4, radial_segments=80, height_segments=1)
    | scale(3, 1, 2.2)
    | trans(30, 0, -2)
  )
  | (cyl(radius=10, height=14, radial_segments=80, height_segments=1)
    | scale(3, 1, 2.2)
    | trans(30, -9, 0)
    | warp(|v| {
       dist = abs(v.x - 30);
       dist = max(dist, 8.) - 8;
       v + vec3(0, 0, -dist * 0.1)
    })
  )
  | simplify(tolerance=0.04)
  | render
