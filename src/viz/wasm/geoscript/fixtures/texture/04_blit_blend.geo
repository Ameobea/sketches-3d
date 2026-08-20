n = 64
base = texture(n, n, |uv| v3(uv.x * 0.5, 0.2, uv.y * 0.5))
sprite = texture(16, 16, |uv| {
  d = len((uv - 0.5) * 2.)
  a = 1. - smoothstep(0.7, 1., d)
  v4(1. - d, d, 0.3, a)
})
blit(sprite | scale(0.25) | trans_global(0.4, 0.3), base, blend="over") | render_texture(name="over")
blit(sprite | scale(0.25) | trans_global(0.95, 0.2), base, blend="add") | render_texture(name="add_wrapped")
blit(sprite | scale(0.25) | trans_global(0.6, 0.65), base, blend="over", filter="nearest") | render_texture(name="over_nearest")
blit(sprite | scale(0.25) | trans_global(0.2, 0.7), base, blend="max") | render_texture(name="max")

// large stamp scaled down: rho > 1 exercises the premultiplied mip path
big = texture(48, 48, |uv| v4(uv.x, uv.y, 1. - uv.x, 1.))
blit(big | scale(0.1) | trans_global(0.7, 0.7), base, blend="over") | render_texture(name="mip_downscale")
