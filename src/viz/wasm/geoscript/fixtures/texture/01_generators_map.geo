n = 64
g = texture(n, n, |uv| {
  y = cos(uv.x * pi * 6.) * sin(uv.y * pi * 4.)
  mix(0.5, remap(-1., 1., 0., 1., y), sigmoid(y * 3.))
})
g | render_texture(name="gen_float")

texture(n, n, |uv, x_ix, y_ix| v3(uv.x, uv.y, float((x_ix + y_ix) % 2))) | render_texture(name="gen_ix")

texture(n, n, |uv| 0.25) | render_texture(name="gen_uniform")

texture(n, n, |uv| v2(fract(uv.x * 3.), uv.y), wrap="clamp") | render_texture(name="gen_vec2_clamp")

(g -> |h, uv| 1. - smoothstep(0.2, 0.8, h) * uv.y) | render_texture(name="mapped")
