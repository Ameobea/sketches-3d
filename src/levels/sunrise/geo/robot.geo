// Player character: a body on three legs, plus the bone chains the engine animates.
// Chain 0 is the body (root); each leg chain starts on the body's abdomen joint.

body = cyl(1, 2.5, 16, 1)
  - (cyl(1, 2, 16, 1)
       | sub(b=cyl(0.8, 2, 16, 1))
       | trans(0, 2.12, 0)
    )
  - (cyl(0.93, 2, 16, 1)
       | trans(0, -2.208, 0)
    )
neck = cyl(0.22, 1.3, 16, 1) + v3(0, 1.5, 0)
head = cyl(0.6, 1, 6, 1)
  -> |v: v3| if v.y > 0 { v * v3(0.8, 1, 0.8) } else { v }
  | trans(0, 2.24, 0)

leg_r = 0.78
leg_len = 4.4
leg_y = -3.4
hip_y = leg_y + leg_len / 2
foot_y = leg_y - leg_len / 2
knee_y = -3.2
leg_dir = |i: int| {
  a = i / 3 * tau
  v3(cos(a), 0, sin(a))
}

legs = 0..3
  -> |i: int| {
    leg = cyl(0.1, leg_len, 4, 1) | (cyl(0.3, 0.15, 3, 1) - v3(0, 2.25, 0))
    leg + (leg_dir(i) * leg_r + v3(0, leg_y, 0))
  }
  | join

export mesh = (body | neck | head | legs)
  | remesh_planar_patches
  | subdivide_by_plane(v3(0, 1, 0), knee_y)

abdomen = v3(0, -0.9, 0)
head_top = v3(0, 2.74, 0)
// The hip joint sits below the leg top so the body's bottom rim is nearer the rigid
// abdomen→hip span than the swinging thigh; the leg's top 0.3 stays fixed like a socket.
hip_joint_y = hip_y - 0.3
leg_chain = |i: int| {
  d = leg_dir(i) * leg_r
  [abdomen, d + v3(0, hip_joint_y, 0), d + v3(0, knee_y, 0), d + v3(0, foot_y, 0)]
}
export bones = [[head_top, abdomen], *(0..3 -> leg_chain)]
