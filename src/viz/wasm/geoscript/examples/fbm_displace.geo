box(5)
  | tess(target_edge_length=0.1)
  | warp(|v, norm| {
    v + norm * ((fbm(v * 0.1) * 0.5 + 0.5) * 2.)
  })
  | simplify(tolerance=0.02)
  | render
