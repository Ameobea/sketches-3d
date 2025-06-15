verts = [vec3(0, 0, 0), vec3(0, 0, 1), vec3(1, 0, 1), vec3(1, 0, 0)]
indices = [1,3,0, 1,2,3]
plane = |size: num| mesh(verts -> mul(b=size), indices)

terrain = plane(300)
  | tess(target_edge_length=1)
  | warp(|v| vec3(v.x, fbm(octaves=8, pos=v*0.01, lacunarity=1.8) * 36, v.z))

terrain | render
