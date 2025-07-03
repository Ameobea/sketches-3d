leg = cyl(radius=4, height=10, radial_segments=32, height_segments=3)
  -> |v| {
    shrink_factor = 1 - linearstep(-6, 6, v.y)
    v * v3(1 - 0.5 * shrink_factor, 1, 1 - 0.5 * shrink_factor)
  }


(leg | rot(pi,0,0) | trans(0,1.4,0))
  | (leg | rot(pi,0,pi-pi/3.3) | rot(0,0,0)      | trans(-cos(0*pi/3)*4.,-6.4,-sin(0*pi/3)*4.))
  | (leg | rot(pi,0,pi-pi/3.3) | rot(0,2*pi/3,0) | trans(-cos(4*pi/3)*4.,-6.4,-sin(4*pi/3)*4.))
  | (leg | rot(pi,0,pi-pi/3.3) | rot(0,4*pi/3,0) | trans(-cos(2*pi/3)*4.,-6.4,-sin(2*pi/3)*4.))
  | render
