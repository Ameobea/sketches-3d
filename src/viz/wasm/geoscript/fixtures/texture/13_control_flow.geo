// Early returns (desugared into nested conditionals) + compile-time-unrolled loops.
n = 32
h = texture(n, n, |uv| fbm(octaves=3, frequency=3., pos=uv, seed=5))
rgb = texture(n, n, |uv| v3(uv.x, uv.y, fbm(pos=uv * 4.)))
w = [0.5, 0.25, 0.125]
k = 0.3
helper = |x| {
  if x > 0.2 { return x * 2. }
  x - 1.0
}

early_return = |v| {
  if v > 0.2 { return 1. }
  v * 2.0
}
ladder = |v| {
  if v > 0.3 { return 3. } else if v > 0.1 { return 2. }
  if v < -0.1 { return 0. }
  v
}
nested_return = |v| {
  if v > 0. {
    if v > 0.25 { return v * 3. }
    y = v * 2.0
    if y > 0.3 { return y }
  }
  v
}
vec_return = |c| {
  if len(c) > 1. { return c * 0.5 }
  c.bgr
}
assign_return = |v| {
  y = if v > 0.1 { return k } else { v * 4. }
  y + 1.0
}
(h -> early_return) | render_texture(name="early_return")
(h -> ladder) | render_texture(name="ladder")
(h -> nested_return) | render_texture(name="nested_return")
(h -> |v| helper(v) + k) | render_texture(name="helper_return")
(rgb -> vec_return) | render_texture(name="vec_return")
(h -> assign_return) | render_texture(name="assign_return")
guarded = |i| {
  h -> |v| {
    if i == 0 { return v }
    v * w[i - 1]
  }
}
(0..4 -> guarded) | render_texture_stack(name="uniform_return_slices")

octaves = |v| fold(0., |acc, o| { acc + sin(v * pow(2., float(o))) * pow(0.5, float(o)) }, 0..4)
(h -> octaves) | render_texture(name="fold_octaves")
(h -> |v| (0..4 -> |o| { sin(v * float(o + 1)) }) | reduce(add)) | render_texture(name="map_reduce")
(rgb -> |c| [c.x, c.y, c.z] | reduce(max)) | render_texture(name="array_reduce")
(h -> |v| fold(v, |acc, i| { acc * w[i] }, 0..3)) | render_texture(name="captured_weights")
(h -> |v| (0..3 -> |o| { if o == 0 { v } else { v * w[o - 1] } }) | reduce(add)) | render_texture(name="guard_in_loop")
(h -> |v| reduce(|acc, x, i| { acc + x * float(i) }, [v, v * 2., v * 3.])) | render_texture(name="reduce_index")
any_all = |v| {
  a = any(|x| x > 0.2, [v, v * 2.])
  b = all(|x| x > -0.1, [v, v * 2.])
  if a && b { 1. } else if a { 0.5 } else { 0. }
}
(h -> any_all) | render_texture(name="any_all")
structural = |v| {
  s = [v, v * 2., v * 3., v * 4.]
  s[1] + (s | reverse)[0] + (s | take(2) | last) + (s | skip(3) | first)
}
(h -> structural) | render_texture(name="structural_ops")
sized = |m| {
  h -> |v| (0..m -> |o| { v * float(o) }) | reduce(add)
}
(2..5 -> sized) | render_texture_stack(name="pinned_bounds")
texture(n, n, |uv| fold(0., |acc, o| { acc + fbm(pos=uv * pow(2., float(o))) * pow(0.5, float(o)) }, 0..3)) | render_texture(name="generator_fold")
