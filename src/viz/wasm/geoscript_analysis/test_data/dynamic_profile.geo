count = 8
contours = 0..count
  -> |i: int| {
    t = i/(count-1)
    pts = 0..400
      -> |i: int| {
        base = v2(i * 0.035, sin(t * pi * 2 + i * 0.025) * 2)
        offset = v2(randf(-1, 1), randf(-1, 1))
        base + offset
      }
    alpha_wrap_2d(points=pts, alpha=0.001, offset=0.025)
      | subpaths
      | sort(by=|p| -len(path_segments(p)))
      | first
      | scale(1.5 + 0.5*t, 1 + 2.5*t)
  }
  | collect
contours = [contours[0], *contours, contours[len(contours) - 1]]
rail_sweep(
  spine_resolution=150,
  spine=|u: float| v3(0, cos(u*pi*0.5), sin(u*pi*0.5))*35,
  dynamic_profile = |u: float| {
    n = len(contours)-2
    s = min(int(floor(u * n)), n - 1)
    t = u * n - s
    a = lerp_paths(contours[s], contours[s + 1], (t + 1) / 2)
    b = lerp_paths(contours[s + 1], contours[s + 2], t / 2)
    p = lerp_paths(a, b, t)
    p = path_union(p, p)
    p
      | subpaths
      | sort(by=|p| -len(path_segments(p)))
      | first
  },
  crease_angle_threshold_deg=nil,
  ring_resolution=450,
  split_seams=true
)
  | remesh_planar_patches(least_squares=true, max_angle_deg=8)
  | render

