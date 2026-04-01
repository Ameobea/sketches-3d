p = 'M7.8 24Q6.38 24 5.72 23.21Q5.06 22.42 5.06 21.17L5.06 13.25L0.94 13.25L0.94 11.62L4.01 11.62Q4.63 11.62 4.88 11.36Q5.14 11.11 5.14 10.49L5.14 -2.25L6.98 -2.25L6.98 11.62L18.62 11.62L18.62 13.25L6.98 13.25L6.98 22.37L24.42 22.37L24.42 24Z'
  | trace_svg_path
  | offset_path(delta=1, join_type='superellipse', superellipse_exponent=4)
  | offset_path(delta=-0.3, join_type='superellipse', superellipse_exponent=4)

m = p
  | tessellate_path
  | extrude(up=v3(0,1,0))
  | remesh_planar_patches

export mesh = m
  - (
    p
      | offset_path(delta=-1, join_type='superellipse', superellipse_exponent=4)
      | tessellate_path
      | extrude(up=v3(0,3,0))
      | remesh_planar_patches
  )
  | origin_to_geometry
