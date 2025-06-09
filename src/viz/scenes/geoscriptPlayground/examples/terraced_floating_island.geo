icosphere(10,7)
  | warp(|v, norm| {
    v = v + norm * fix_float(pow((fbm(v*0.07) * 0.5 + 0.5) * 4, 1.6));
    vec3(v.x, round(v.y), v.z)
  })
  | simplify(tolerance=0.005)
  | render
