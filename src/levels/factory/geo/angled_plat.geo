p = trace_path(|| {
  circle(radius=35, center=v2(0))
})
p = path_intersect(
  p,
  trace_path(|| {
    move(-2, -12)
    line(2, -12)
    line(2, 12)
    line(-2, 12)
    close()
  }
)
  | path_scale(4, 3)
  | path_trans(0, -35))
  | path_trans(0, 25)

export mesh = p
  | tessellate_path
  | extrude(v3(0, 2, 0))
  -> |v| {
    if v.z > 0 && v.y > 0 {
      v3(v.x, v.y + 2, v.z)
    } else if (v.z < 0) {
      v3(v.x * 0.4, v.y - (if v.y > 0 { 5 } else { 4.25 }), v.z)
    } else {
      v
    }
  }
  | remesh_planar_patches
  | scale(0.5)
