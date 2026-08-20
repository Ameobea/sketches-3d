set_rng_seed(7)
n = 96
s = 12.
sprite = texture(12, 12, |uv| {
  d = len((uv - 0.5) * 2.)
  v2(1. - smoothstep(0., 1., d), 1. - smoothstep(0.8, 1., d))
})
place = || (floor(randf() * n) + s * 0.5) / n
field = scatter(40, |ix| sprite | scale(s / n) | trans_global(place(), place()), texture(n, n, |uv| 0.), blend="over", filter="nearest")
field | render_texture(name="scattered")
