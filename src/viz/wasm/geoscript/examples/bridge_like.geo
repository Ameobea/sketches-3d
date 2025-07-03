pillar = box(2.8,34,2.8) + v3(0,17,0)

bar = box(25,3,2.8)
  | tess(target_edge_length=6)
  -> |v| {
    if v.x+25/2 > 5 && v.x+25/2 < 20 {
      return v
    }

    v3(v.x, if v.y < 0 { -3.6 } else { v.y }, v.z)
  };

bars = (bar + v3(25/2,16,0))
  | (bar + v3(25/2,28,0));

pillars = pillar + (pillar + v3(25,0,0))

segment = pillars
  | bars
  | (bars | scale(0.8,1,1) | rot(0,-pi/2,0))
  | (bars | scale(0.8,1,1) | rot(0,-pi/2,0) | trans(25,0,0))

0..4
  -> |i| { segment | trans(0,0,0.8*25*i) }
  | union
  | (pillars + v3(0,0,0.8*25*4))
  | (bars + v3(0,0,0.8*25*4))
  | simplify(tolerance=0.1)
  | origin_to_geometry
  | render
