contours = 0..20 -> |layer_ix| {
  radius = 5 + sin(layer_ix*0.5) - layer_ix * 0.2
  0..30 -> |i| {
    t = i / 30
    vec3(sin(t*pi*2) * radius, layer_ix, cos(t*pi*2) * radius)
  }
}

bottle = contours | stitch_contours(flipped=true, closed=true, cap_ends=true)
bottle | render
