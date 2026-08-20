n = 32
h = texture(n, n, |uv| fbm(octaves=3, frequency=3., pos=uv, seed=5))
rgb = texture(n, n, |uv| v3(uv.x, uv.y, fbm(pos=uv * 4.)))
stripes = texture(n, n, |uv| floor(uv.x * 4.))
k = 0.3
flag = k > 0.

(h -> |v| if v > 0.1 { v * 2. } else { v - 1. }) | render_texture(name="varying_cond")
(h -> |v| if v > 0.3 { 1. } else if v > 0. { 0.5 } else if v > -0.3 { 0.25 } else { 0. }) | render_texture(name="else_if_chain")
(h -> |v| {
  m: bool = v > 0. && v < 0.4
  n = v < -0.2 || !m
  if m == n { v } else if m != n && !n { v * 3. } else { k }
}) | render_texture(name="logic_masks")
(rgb -> |c| if len(c) > 1. { c * 0.5 } else { c.bgr + v3(0.1, 0.2, 0.3) }) | render_texture(name="vec_select")
(h -> |v| if v >= 0.2 { k } else { v }) | render_texture(name="uniform_arm_broadcast")
(stripes -> |x| if x > 0. { 1. / x } else { 0. }) | render_texture(name="inf_in_untaken_arm")
(h -> |v, uv| if flag && v > 0. { v + uv.x } else if flag { v } else { 0. }) | render_texture(name="uniform_lhs_logic")

slices = 0..4 -> |i| (h -> |v| if i == 0 { v } else { 1. - smoothstep(i * 0.1, i * 0.1 + 0.4, v) })
slices | render_texture_stack(name="uniform_cond_slices")
w = [0.5, 0.25, 0.125]
(0..4 -> |i| (h -> |v| if i == 0 { v } else { v * w[i - 1] })) | render_texture_stack(name="guarded_arm_fallback")

pick = |m: bool, a, b| if m { a } else { b }
(h -> |v| pick(v <= 0.1, sigmoid(v), abs(v))) | render_texture(name="mask_param_helper")
texture(n, n, |uv| if uv.x > uv.y { uv.x } else { uv.y * 2. }) | render_texture(name="generator_cond")
