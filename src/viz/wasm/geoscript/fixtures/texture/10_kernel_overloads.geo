n = 32
h = texture(n, n, |uv| fbm(octaves=3, frequency=3., pos=uv, seed=5))
g = texture(n, n, |uv| uv.x * 6. - 3.)
rgb = texture(n, n, |uv| v3(uv.x, uv.y, fbm(pos=uv * 4.)))

(sin(g) + cos(h * 10.) * tan(clamp(-1.2, 1.2, g))) | render_texture(name="trig")
(asin(clamp(-1., 1., h)) + acos(clamp(-1., 1., h)) - atan(g)) | render_texture(name="arc")
atan2(h, g) + atan2(h, 0.5) + atan2(0.5, h) | render_texture(name="atan2s")
(sqrt(abs(g)) * exp(-h) + log2(h + 2.)) | render_texture(name="exp_log")
(floor(g) + ceil(h * 3.) - round(g * 2.) + fract(g) - trunc(g)) | render_texture(name="rounding")
sigmoid(g * 2.) | render_texture(name="squashed")
(mod(g, 0.7) + mod(g, h + 1.) - mod(0.9, h + 1.)) | render_texture(name="mods")
negated = -(h - 0.5)
negated | render_texture(name="negated")

lerp(0.25, h, g) | render_texture(name="lerp_const")
lerp(sigmoid(g), rgb, rgb * 0.5) | render_texture(name="lerp_tex_broadcast")

len(rgb) | render_texture(name="length")
normalize(rgb + 0.1) | render_texture(name="normalized_vec")
dot(rgb, rgb) | render_texture(name="dot_self")
distance(rgb, rgb * 0.5) | render_texture(name="dist")

v3(h, g, 0.25) | render_texture(name="constructed")
v2(h, 1.) | render_texture(name="constructed2")
v4(h, g, h * g, 1.) | render_texture(name="constructed4")
v3(h) | render_texture(name="splatted")

mask = texture(n, n, |uv, x, y| float((x + y) % 2))
(rgb * mask + rgb.bgr * (1. - mask)) | render_texture(name="broadcast_checker")
