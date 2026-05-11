radius = 17.8

path = 0..64
  -> |i| {
    t = i/64
    v3(cos(t*tau) * (radius-0.8), 4, sin(t*tau) * (radius-0.8))
  }
  | filter(|x| x != nil)

railing = rail_sweep(
  spine=path,
  spine_resolution=64,
  ring_resolution=4,
  closed=true,
  profile=build_path(path { rect(v2(0), v2(1, 0.6)) })
)
  | trans(0, 7.4, 0)

post = box(1, 12, 1.4)
  | trans(radius-1, 5.6, 0)

posts = 0..32
  -> |i| {
    if i == 24 || i == 8 {
      return nil
    }

    t = i/32
    post | rot_global(0, t*tau, 0)
  }
  | filter(|x| x != nil)
  | join

export mesh = posts | railing
