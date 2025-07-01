b = box(2)
b
  // walks along the mesh's surface up for 1.5 units then left for -1 unit
  | trace_geodesic_path(
      path=[v2(0,1.5), v2(-1, 0)],
      start_pos_local_space=v3(-0.3,10,0.3),
      up_dir_world_space=v3(-1,0,-1)
  )
  | fan_fill(flipped=false, closed=true)
  | extrude(up=v3(-0.5,1.5,0.5))
  | trans(0,-0.1,0)
  | union(b)
  | render
