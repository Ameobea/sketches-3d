wheel_radius = 10
wheel_thickness = 1
hub_radius = 1.5
hub_axle_bore_radius = 0.5
hub_thickness = 1.5
spoke_count = 8
spoke_radius = 0.25

rim_path = 0..40 -> |i| {
  t = i / 39
  angle = t * pi * 2
  vec3(sin(angle) * wheel_radius, 0, cos(angle) * wheel_radius)
}

rim = rim_path | extrude_pipe(radius=wheel_thickness/2, resolution=4, connect_ends=true, twist=pi/4)

hub_solid = cyl(radius=hub_radius, height=hub_thickness, radial_segments=16, height_segments=1)

axle_hole = cyl(radius=hub_axle_bore_radius, height=hub_thickness + 0.1, radial_segments=16, height_segments=3)

hub = hub_solid - axle_hole

spoke_length = wheel_radius

spokes = 0..(spoke_count - 1) -> |i| {
  angle = i * (2*pi / spoke_count)

  spoke_cylinder = cyl(radius=spoke_radius, height=spoke_length*2, radial_segments=8, height_segments=1)

  spoke_cylinder
    | rot(0, 0, pi/2)
    | rot(0, angle, 0)
} | join;

(rim | hub | spokes) | render
