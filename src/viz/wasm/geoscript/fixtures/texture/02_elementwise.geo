n = 64
h = texture(n, n, |uv| fbm(octaves=3, frequency=4., pos=uv, tileable=true))
a = 1. - smoothstep(0.3, 0.7, h)
b = clamp(0., 1., h * 1.5 - 0.2)
c = min(a, b) + max(h, 0.4) * 0.25
(c / 2. + pow(h, 2.)) | render_texture(name="arith")

tint = texture(n, n, |uv| v3(uv.x, uv.y, 0.5)) * v3(1., 0.6, 0.3)
tint | render_texture(name="tinted")

texture_normalize(h) | render_texture(name="normalized")
texture_invert(b) | render_texture(name="inverted")
texture_levels(0.1, 0.9, 0., 1., 1.2, h) | render_texture(name="levels")
remap(0., 1., -0.25, 1.25, h) | render_texture(name="remapped")
