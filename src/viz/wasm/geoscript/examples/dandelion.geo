sph = icosphere(radius=20, resolution=5)
build_spoke = |pos: vec3, norm: vec3|: mesh {
  spoke = cyl(radius=0.2, height=14, radial_segments=10, height_segments=1)
  (spoke + pos + norm*6.99) | look_at(target=pos + norm)
}
spokes = sph | point_distribute(count=380, cb=build_spoke) | union

build_ring = |segment_count: int, ring_radius: num, tube_radius: num = 1|: mesh {
  0..segment_count
    -> |i| {
      t = i / (segment_count - 1)
      vec3(sin(t*pi*2) * ring_radius, 0, cos(t*pi*2) * ring_radius)
    }
    | extrude_pipe(radius=tube_radius, resolution=4, twist=pi/4)
}

ring = build_ring(segment_count=190, ring_radius=20+14, tube_radius=0.2) | scale(1, 8, 1)
rings = 0..12
  -> || { ring | rot(randv(-pi*2, pi*2)) }
  | union;

(sph | spokes | rings) | render
