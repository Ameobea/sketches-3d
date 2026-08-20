// Adapted from the motivating composition in docs/texture-autovec-plan.md (self-contained,
// reduced res/layers). The two closures are the autovec acceptance surface.
n = 128
height = texture(n, n, |uv| {
  y = cos(uv.x * pi * 38.)
  mix(0.5, remap(-1., 1., 0., 1., y), sigmoid(y * 3.))
})
dirt = texture(n, n, |uv| v3(0.3 + 0.2 * fbm(octaves=3, frequency=6., pos=uv, tileable=true), 0.22, 0.15))
base = texture(n, n, |uv| v3(0.5, 0.45, 0.4))

layers = 0..6 -> |i| {
  mask = height -> |h| 1. - smoothstep(i * 0.07, i * 0.07 + 0.4, h)
  blit(concat_channels(dirt, mask), base, blend="over")
}
layers | render_texture_stack(name="layers", usage="albedo")

height | render_texture(name="height", usage="height")
(height | height_to_normal(2.)) | render_texture(name="normal", usage="normal")
