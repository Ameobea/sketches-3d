n = 48
accum = texture(n, n, |uv| fbm(octaves=4, frequency=4., pos=uv, tileable=true))
slice = |t: float| {
  lo = 0.78 - 0.72 * t
  accum -> |a: float, uv: vec2| {
    base_c = mix(a, v3(0.5, 0.49, 0.46), v3(0.67, 0.65, 0.61))
    dirt_c = mix(t, v3(0.34, 0.26, 0.14), v3(0.2, 0.15, 0.09))
    mix(smoothstep(lo, lo + 0.17, a), base_c, dirt_c)
  }
}
(0..5 -> |i| slice(i / 4.)) | render_texture_stack(name="stack", usage="albedo")
