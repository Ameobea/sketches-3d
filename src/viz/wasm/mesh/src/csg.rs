//! A Rust port of the CSG.js library by Evan Wallace
//! (https://github.com/evanw/csg.js/).

use std::ops::BitOr;

use slotmap::new_key_type;
pub use slotmap::SlotMap;

use crate::{linked_mesh::Vec3, LinkedMesh, Triangle};

const EPSLION: f32 = 1e-5;

#[derive(Clone)]
pub struct Plane {
  pub normal: Vec3,
  pub w: f32,
}

pub enum Coplanars<'a> {
  UseFrontBack,
  SingleBuffer(&'a mut Vec<Polygon>),
}

impl<'a> Coplanars<'a> {
  pub fn push_front(&mut self, polygon: Polygon, front: &mut Vec<Polygon>) {
    match self {
      Coplanars::UseFrontBack => front.push(polygon),
      Coplanars::SingleBuffer(buffer) => {
        buffer.push(polygon);
      }
    }
  }

  pub fn push_back(&mut self, polygon: Polygon, back: &mut Vec<Polygon>) {
    match self {
      Coplanars::UseFrontBack => back.push(polygon),
      Coplanars::SingleBuffer(buffer) => {
        buffer.push(polygon);
      }
    }
  }
}

impl Plane {
  pub fn flip(&mut self) {
    self.normal = -self.normal;
    self.w = -self.w;
  }

  pub fn from_points(a: Vec3, b: Vec3, c: Vec3) -> Self {
    let normal = (b - a).cross(&(c - a)).normalize();
    let w = normal.dot(&a);
    Self { normal, w }
  }

  /// Split `polygon` by this plane if needed, then put the polygon or polygon
  /// fragments in the appropriate lists.
  ///
  /// Coplanar polygons go into either `coplanar_front` or `coplanar_back`
  /// depending on their orientation with respect to this plane. Polygons in
  /// front or in back of this plane go into either `front` or `back`.
  pub fn split_polygon(
    &self,
    polygon: &Polygon,
    mut coplanars: Coplanars<'_>,
    front: &mut Vec<Polygon>,
    back: &mut Vec<Polygon>,
  ) {
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum PolygonClass {
      Coplanar = 0,
      Front = 1,
      Back = 2,
      #[allow(dead_code)]
      Spanning = 3,
    }

    impl From<u8> for PolygonClass {
      fn from(val: u8) -> Self {
        match val {
          0 => PolygonClass::Coplanar,
          1 => PolygonClass::Front,
          2 => PolygonClass::Back,
          3 => PolygonClass::Spanning,
          _ => panic!("Invalid PolygonClass value"),
        }
      }
    }

    impl BitOr for PolygonClass {
      type Output = Self;

      fn bitor(self, rhs: Self) -> Self::Output {
        let out = self as u8 | rhs as u8;
        out.into()
      }
    }

    let mut polygon_type = PolygonClass::Coplanar;
    let mut types = Vec::with_capacity(3);
    for vertex in polygon.vertices.iter() {
      let t = self.normal.dot(&vertex.pos) - self.w;
      let polygon_class = if t < -EPSLION {
        PolygonClass::Back
      } else if t > EPSLION {
        PolygonClass::Front
      } else {
        PolygonClass::Coplanar
      };

      polygon_type = polygon_type | polygon_class;
      types.push(polygon_class);
    }

    // Put the polygon in the correct list, splitting it when necessary.
    match polygon_type {
      PolygonClass::Coplanar => {
        if self.normal.dot(&polygon.plane.normal) > 0. {
          coplanars.push_front(polygon.clone(), front);
        } else {
          coplanars.push_back(polygon.clone(), back);
        }
      }
      PolygonClass::Front => {
        front.push(polygon.clone());
      }
      PolygonClass::Back => {
        back.push(polygon.clone());
      }
      PolygonClass::Spanning => {
        let mut f = Vec::with_capacity(3);
        let mut b = Vec::with_capacity(3);
        for i in 0..polygon.vertices.len() {
          let j = (i + 1) % polygon.vertices.len();
          let ti = types[i];
          let tj = types[j];
          let vi = &polygon.vertices[i];
          let vj = &polygon.vertices[j];

          if ti != PolygonClass::Back {
            f.push(vi.clone());
          }
          if ti != PolygonClass::Front {
            b.push(if ti != PolygonClass::Back {
              vi.clone()
            } else {
              vi.clone()
            });
          }
          if (ti | tj) == PolygonClass::Spanning {
            let t = (self.w - self.normal.dot(&vi.pos)) / self.normal.dot(&(vj.pos - vi.pos));
            let v = vi.interpolate(vj, t);
            f.push(v.clone());
            b.push(v);
          }
        }

        if f.len() >= 3 {
          front.push(Polygon::new(f));
        }
        if b.len() >= 3 {
          back.push(Polygon::new(b));
        }
      }
    }
  }
}

#[derive(Clone)]
pub struct Vertex {
  pub pos: Vec3,
  pub normal: Vec3,
}

impl Vertex {
  pub fn flip(&mut self) {
    self.normal = -self.normal;
  }

  fn interpolate(&self, vj: &Vertex, t: f32) -> Self {
    Vertex {
      pos: self.pos.lerp(&vj.pos, t),
      normal: self.normal.lerp(&vj.normal, t).normalize(),
    }
  }
}

#[derive(Clone)]
pub struct Polygon {
  pub vertices: Vec<Vertex>,
  pub plane: Plane,
}

impl Polygon {
  pub fn new(vertices: Vec<Vertex>) -> Self {
    assert!(vertices.len() >= 3);
    let plane = Plane::from_points(vertices[0].pos, vertices[1].pos, vertices[2].pos);
    Self { vertices, plane }
  }

  pub fn flip(&mut self) {
    self.vertices.reverse();
    for vtx in &mut self.vertices {
      vtx.flip();
    }
    self.plane.flip();
  }
}

new_key_type! {
  pub struct NodeKey;
}

pub type NodeMap = SlotMap<NodeKey, Node>;

#[derive(Clone)]
pub struct Node {
  pub plane: Option<Plane>,
  pub front: Option<NodeKey>,
  pub back: Option<NodeKey>,
  pub polygons: Vec<Polygon>,
}

impl Node {
  /// Convert solid space to empty space and empty space to solid space.
  pub fn invert(self_key: NodeKey, nodes: &mut NodeMap) {
    let (front, back) = {
      let this = &mut nodes[self_key];
      for polygon in &mut this.polygons {
        polygon.flip();
      }
      if let Some(plane) = &mut this.plane {
        plane.flip();
      }
      (this.front, this.back)
    };
    if let Some(front_key) = front {
      Node::invert(front_key, nodes);
    }
    if let Some(back_key) = back {
      Node::invert(back_key, nodes);
    }

    let this = &mut nodes[self_key];
    std::mem::swap(&mut this.front, &mut this.back);
  }

  pub fn clip_polygons(
    self_key: NodeKey,
    mut polygons: Vec<Polygon>,
    nodes: &NodeMap,
  ) -> Vec<Polygon> {
    let this = &nodes[self_key];
    let Some(plane) = &this.plane else {
      return polygons;
    };

    let mut front = Vec::new();
    let mut back = Vec::new();

    for polygon in &mut polygons {
      plane.split_polygon(polygon, Coplanars::UseFrontBack, &mut front, &mut back);
    }

    if let Some(front_key) = this.front {
      front = Node::clip_polygons(front_key, front, nodes);
    }
    if let Some(back_key) = this.back {
      back = Node::clip_polygons(back_key, back, nodes);
    } else {
      back = Vec::new();
    }

    front.extend(back);
    front
  }

  // Recursively remove all polygons in `polygons` that are inside this BSP tree.
  pub fn clip_to(self_key: NodeKey, bsp_key: NodeKey, nodes: &mut NodeMap) {
    let this_polygons = {
      let this = &mut nodes[self_key];
      std::mem::take(&mut this.polygons)
    };
    let new_this_polygons = Node::clip_polygons(bsp_key, this_polygons, nodes);
    {
      let this = &mut nodes[self_key];
      this.polygons = new_this_polygons;
    }

    if let Some(front_key) = nodes[self_key].front {
      Node::clip_to(front_key, bsp_key, nodes);
    }
    if let Some(back_key) = nodes[self_key].back {
      Node::clip_to(back_key, bsp_key, nodes);
    }
  }

  /// Returns a list of all polygons in this BSP tree.
  pub fn all_polygons(self_key: NodeKey, nodes: &NodeMap) -> Vec<Polygon> {
    let (mut polygons, this_front, this_back) = {
      let this = &nodes[self_key];
      let mut polygons = Vec::with_capacity(this.polygons.len());
      polygons.extend(this.polygons.iter().cloned());
      (polygons, this.front, this.back)
    };

    if let Some(front_key) = this_front {
      polygons.extend(Node::all_polygons(front_key, nodes));
    }
    if let Some(back_key) = this_back {
      polygons.extend(Node::all_polygons(back_key, nodes));
    }

    polygons
  }

  /// Build a BSP tree out of `polygons`. Each set of polygons is partitioned
  /// using the first polygon (no heuristic is used to pick a good split).
  pub fn build(polygons: &[Polygon], nodes: &mut NodeMap) -> Self {
    if polygons.is_empty() {
      panic!("No polygons provided to build BSP tree");
    }
    let plane = polygons[0].plane.clone();
    let mut front = Vec::new();
    let mut back = Vec::new();
    let mut this_polygons = Vec::new();
    for polygon in polygons {
      plane.split_polygon(
        polygon,
        Coplanars::SingleBuffer(&mut this_polygons),
        &mut front,
        &mut back,
      );
    }

    let front = if front.is_empty() {
      None
    } else {
      Some(Self::build(&front, nodes))
    };
    let front_key = front.map(|node| nodes.insert(node));
    let back = if back.is_empty() {
      None
    } else {
      Some(Self::build(&back, nodes))
    };
    let back_key = back.map(|node| nodes.insert(node));

    Self {
      plane: Some(plane),
      front: front_key,
      back: back_key,
      polygons: this_polygons,
    }
  }

  pub fn add_polygons(self_key: NodeKey, polygons: &[Polygon], nodes: &mut NodeMap) {
    let mut front = Vec::new();
    let mut back = Vec::new();
    let (front_key, back_key) = {
      let this = &mut nodes[self_key];
      for polygon in polygons {
        this.plane.as_ref().unwrap().split_polygon(
          polygon,
          Coplanars::SingleBuffer(&mut this.polygons),
          &mut front,
          &mut back,
        );
      }
      (this.front, this.back)
    };

    if !front.is_empty() {
      match front_key {
        Some(front_key) => Node::add_polygons(front_key, &front, nodes),
        None => {
          let new_front = Self::build(&front, nodes);
          let new_front_key = Some(nodes.insert(new_front));
          nodes[self_key].front = new_front_key;
        }
      }
    }
    if !back.is_empty() {
      match back_key {
        Some(back_key) => Node::add_polygons(back_key, &back, nodes),
        None => {
          let new_back = Self::build(&back, nodes);
          let new_back_key = Some(nodes.insert(new_back));
          nodes[self_key].back = new_back_key;
        }
      }
    }
  }
}

pub struct CSG {
  pub polygons: Vec<Polygon>,
}

impl CSG {
  pub fn new(polygons: Vec<Polygon>) -> Self {
    Self { polygons }
  }

  pub fn into_polygons(self) -> Vec<Polygon> {
    self.polygons
  }

  pub fn iter_triangles<'a>(&'a self) -> impl Iterator<Item = Triangle> + 'a {
    self.polygons.iter().flat_map(|polygon| {
      // \/ Not sure if this will be valid
      (2..polygon.vertices.len()).map(move |i| {
        Triangle::new(
          polygon.vertices[0].pos,
          polygon.vertices[i - 1].pos,
          polygon.vertices[i].pos,
        )
      })
    })
  }

  pub fn to_linked_mesh(&self) -> LinkedMesh {
    LinkedMesh::from_triangles(self.iter_triangles())
  }

  /// Return a new CSG solid representing space in either this solid or in the
  /// solid `csg`. Neither this solid nor the solid `csg` are modified.
  ///
  ///     A.union(B)
  ///
  ///     +-------+            +-------+
  ///     |       |            |       |
  ///     |   A   |            |       |
  ///     |    +--+----+   =   |       +----+
  ///     +----+--+    |       +----+       |
  ///          |   B   |            |       |
  ///          |       |            |       |
  ///          +-------+            +-------+
  pub fn union(self, csg: CSG, nodes: &mut NodeMap) -> Self {
    let a = Node::build(&self.polygons, nodes);
    let a_key = nodes.insert(a);
    let b = Node::build(&csg.polygons, nodes);
    let b_key = nodes.insert(b);

    Node::clip_to(a_key, b_key, nodes);
    Node::clip_to(b_key, a_key, nodes);
    Node::invert(b_key, nodes);
    Node::clip_to(b_key, a_key, nodes);
    Node::invert(b_key, nodes);
    Node::add_polygons(a_key, &Node::all_polygons(b_key, nodes), nodes);
    Self::new(Node::all_polygons(a_key, nodes))
  }

  /// Returns a new CSG solid representing space in this solid but not in the
  /// solid `csg`. Neither this solid nor the solid `csg` are modified.
  ///
  ///    A.subtract(B)
  ///
  ///    +-------+            +-------+
  ///    |       |            |       |
  ///    |   A   |            |       |
  ///    |    +--+----+   =   |    +--+
  ///    +----+--+    |       +----+
  ///         |   B   |
  ///         |       |
  ///         +-------+
  pub fn subtract(self, csg: CSG, nodes: &mut NodeMap) -> Self {
    let a = Node::build(&self.polygons, nodes);
    let a_key = nodes.insert(a);
    let b = Node::build(&csg.polygons, nodes);
    let b_key = nodes.insert(b);

    Node::invert(a_key, nodes);
    Node::clip_to(a_key, b_key, nodes);
    Node::clip_to(b_key, a_key, nodes);
    Node::invert(b_key, nodes);
    Node::clip_to(b_key, a_key, nodes);
    Node::invert(b_key, nodes);
    Node::add_polygons(a_key, &Node::all_polygons(b_key, nodes), nodes);
    Self::new(Node::all_polygons(a_key, nodes))
  }

  /// Return a new CSG solid representing space both this solid and in the
  /// solid `csg`. Neither this solid nor the solid `csg` are modified.
  ///
  ///    A.intersect(B)
  ///
  ///   +-------+
  ///   |       |
  ///   |   A   |
  ///   |    +--+----+   =   +--+
  ///   +----+--+    |       +--+
  ///        |   B   |
  ///        |       |
  ///        +-------+
  pub fn intersect(self, csg: CSG, nodes: &mut NodeMap) -> Self {
    let a = Node::build(&self.polygons, nodes);
    let a_key = nodes.insert(a);
    let b = Node::build(&csg.polygons, nodes);
    let b_key = nodes.insert(b);

    Node::invert(a_key, nodes);
    Node::clip_to(a_key, b_key, nodes);
    Node::invert(b_key, nodes);
    Node::clip_to(b_key, a_key, nodes);
    Node::invert(a_key, nodes);
    Node::add_polygons(a_key, &Node::all_polygons(b_key, nodes), nodes);
    Self::new(Node::all_polygons(a_key, nodes))
  }

  /// Return a new CSG solid with solid and empty space switched. This solid is
  /// not modified.
  pub fn inverse(&self) -> Self {
    let mut polygons = Vec::with_capacity(self.polygons.len());
    for polygon in &self.polygons {
      let mut polygon = polygon.clone();
      polygon.flip();
      polygons.push(polygon);
    }
    Self::new(polygons)
  }

  /// Construct an axis-aligned solid cuboid. Optional parameters are `center`
  /// and `radius`, which default to `[0, 0, 0]` and `[1, 1, 1]`.
  pub fn new_cube(center: Vec3, radius: f32) -> Self {
    let polygons: [[[i32; 3]; 4]; 6] = [
      [[-1, -1, -1], [-1, -1, 1], [-1, 1, 1], [-1, 1, -1]], // Left face
      [[1, -1, -1], [1, 1, -1], [1, 1, 1], [1, -1, 1]],     // Right face
      [[-1, -1, -1], [1, -1, -1], [1, -1, 1], [-1, -1, 1]], // Bottom face
      [[-1, 1, -1], [-1, 1, 1], [1, 1, 1], [1, 1, -1]],     // Top face
      [[-1, -1, -1], [-1, 1, -1], [1, 1, -1], [1, -1, -1]], // Back face
      [[-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]],     // Front face
    ];

    let mut vertices = Vec::new();
    for face_vertices in &polygons {
      let mut polygon_vertices = Vec::new();
      let normal = Plane::from_points(
        Vec3::new(
          face_vertices[0][0] as f32,
          face_vertices[0][1] as f32,
          face_vertices[0][2] as f32,
        ),
        Vec3::new(
          face_vertices[1][0] as f32,
          face_vertices[1][1] as f32,
          face_vertices[1][2] as f32,
        ),
        Vec3::new(
          face_vertices[2][0] as f32,
          face_vertices[2][1] as f32,
          face_vertices[2][2] as f32,
        ),
      )
      .normal;
      for vtx in face_vertices {
        let pos = Vec3::new(
          center[0] + radius * vtx[0] as f32,
          center[1] + radius * vtx[1] as f32,
          center[2] + radius * vtx[2] as f32,
        );
        polygon_vertices.push(Vertex { pos, normal });
      }
      vertices.push(Polygon::new(polygon_vertices));
    }

    Self::new(vertices)
  }
}
