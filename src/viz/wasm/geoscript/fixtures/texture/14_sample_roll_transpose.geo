n = 48
src = texture(n, n, |uv| v3(uv.x, uv.y, fbm(pos=uv * 3.)))
g = src.r

// O(1) views + exact toroidal shift
transpose(src) | render_texture(name="transpose")
texture_transpose(src.bgr)[4..20, 8..40] | render_texture(name="transpose_view_chain")
texture_roll(7, -3, src) | render_texture(name="roll")
texture_roll(0, n, g) | render_texture(name="roll_identity")

// gathers: every filter/wrap combination, fed by dense and view sources
texture(n, n, |uv| sample(src, uv + v2(0.05 * sin(uv.y * tau), 0.))) | render_texture(name="warp_bilinear_repeat")
texture(n, n, |uv| sample(src, uv * 1.5 - 0.25, filter="nearest", wrap="clamp")) | render_texture(name="warp_nearest_clamp")
texture(n, n, |uv| sample(flip_x(src), v2(uv.y, uv.x) * 2., wrap="mirror")) | render_texture(name="warp_mirror_view")
texture(n, n, |uv| sample(transpose(g), uv.yx * 3. - 1., filter="nearest", wrap="mirror")) | render_texture(name="warp_nearest_mirror_transposed")
texture(n, n, |uv| sample(g, v2(uv.x, 0.5), filter="nearest")) | render_texture(name="uniform_coord")

// per-row offsets from a strip, the stretch-about-center idiom, and a displacement map
offs = texture(1, n, |uv| fract(sin(uv.y * 91.7) * 43758.5))
[src, resize(n, n, offs, filter="nearest")] | texture_zip(|p, o, uv| sample(src, v2(uv.x + o, uv.y), filter="nearest"))
  | render_texture(name="row_roll")
texture(n, n, |uv| sample(src, v2((uv.x - 0.5) * (1. + 0.3 * sin(uv.y * tau)) + 0.5, uv.y))) | render_texture(name="row_stretch")
(src -> |p, uv| sample(src, uv + (p.rg - 0.5) * 0.1)) | render_texture(name="displace")

// same body over 1ch and 3ch sources (plan-per-channel-count), plus nearest identity
warp = |t| texture(n, n, |uv| sample(t, uv * 2.))
warp(g) | render_texture(name="helper_1ch")
warp(src) | render_texture(name="helper_3ch")
texture(n, n, |uv| sample(src, uv, filter="nearest")) | render_texture(name="nearest_identity")
