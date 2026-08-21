n = 32
a = texture(n, n, |uv| v4(uv.x, uv.y, fbm(pos=uv * 3.), fbm(pos=uv * 5., seed=2)))
b = texture(n, n, |uv| v4(fbm(pos=uv * 2.), uv.y, uv.x, fbm(pos=uv * 7., seed=3)))
rgb0 = texture(n, n, |uv| v3(uv.x, uv.y, 0.25))
rgb1 = texture(n, n, |uv| v3(fbm(pos=uv * 4.), uv.x, uv.y))
mask = texture(n, n, |uv| fbm(pos=uv * 6., seed=9))
k = 0.45

blend = |t0: vec4, t1: vec4|: vec4 {
  if t0.a > 0.7 {
    t0
  } else if t1.a < 0.3 {
    t0 * 0.5
  } else {
    v4((t0.rgb + t1.rgb) / 2., 1.)
  }
}
[a, b] | texture_zip(blend) | render_texture(name="conditional_blend")

// mixed arities, and the select shape that stands in for a native `select` builtin
[rgb0, rgb1, mask] | texture_zip(|p, q, m| mix(smoothstep(0.3, 0.7, m), p, q))
  | render_texture(name="masked_blend")
[rgb0, rgb1, mask] | texture_zip(|p, q, m| if m > k { p } else { q })
  | render_texture(name="hard_select")

// uv / trailing params, a uniform-valued body, and the 1-input degenerate case
[mask, rgb0] | texture_zip(|m, c, uv| v3(m) * c + uv.x) | render_texture(name="with_uv")
[mask, rgb0] | texture_zip(|m, c| k) | render_texture(name="uniform_body")
[rgb1] | texture_zip(|c| c.bgr * 2.) | render_texture(name="single_input")

// same body, swapped input arities: the plan cache must not confuse the two
f = |x, y| x * y
[mask, rgb0] | texture_zip(f) | render_texture(name="arity_swap_a")
[rgb0, mask] | texture_zip(f) | render_texture(name="arity_swap_b")

// many inputs, past any single-texture channel limit
[mask, rgb0, rgb1, a, b] | texture_zip(|m, p, q, s, t| v4(p * m + q, 1.) + s * 0.25 + t * 0.125)
  | render_texture(name="five_inputs")
