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
        window = box(5, 3.2, 5) | trans(pos + norm)
        window | look_at(target=center, up=v3(0, 0, -1)) | trans(0, 0.8, 0)
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
  | {
    pts = path(20) | skip(1) | take(19);
    [pts -> sub(b=v3(0,4,0)), pts -> sub(b=v3(0,15,0))]
      | stitch_contours(closed=false, flipped=true)
      | extrude(up=v3(0,0,-3))
  }
  | simplify(tolerance=0.04)
  | render
