//! A Rust port of the CSG.js library by Evan Wallace
//! (https://github.com/evanw/csg.js/).

use std::ops::BitOr;

use fxhash::FxHashMap;
use slotmap::Key;
use smallvec::SmallVec;

use crate::{
  linked_mesh::{self, DisplacementNormalMethod, EdgeSplitPos, FaceKey, Vec3, VertexKey},
  LinkedMesh,
};

const EPSLION: f32 = 1e-5;

slotmap::new_key_type! {
  pub struct NodeKey;
}

pub type NodeMap = slotmap::SlotMap<NodeKey, Node>;

static mut SPLIT_FACE_CACHE: *mut Vec<(FaceKey, [FaceKey; 2])> = std::ptr::null_mut();

fn init_split_face_scratch() {
  unsafe {
    SPLIT_FACE_CACHE = Box::into_raw(Box::new(Vec::new()));
  }
}

fn get_split_face_scratch() -> &'static mut Vec<(FaceKey, [FaceKey; 2])> {
  unsafe {
    if SPLIT_FACE_CACHE.is_null() {
      init_split_face_scratch();
    }
    &mut *SPLIT_FACE_CACHE
  }
}

#[derive(Clone, Debug)]
pub struct Plane {
  pub normal: Vec3,
  pub w: f32,
}

#[derive(Debug)]
pub enum Coplanars {
  UseFrontBack,
  SingleBuffer(NodeKey),
}

impl Coplanars {
  pub fn push_front(
    &self,
    polygon: Polygon,
    front_key: NodeKey,
    nodes: &mut NodeMap,
    // TODO: should probably replace this with user data on `LinkedMesh` faces
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    match self {
      Coplanars::UseFrontBack => {
        node_key_by_face_key.insert(polygon.key, front_key);
        let front = &mut nodes[front_key].polygons;
        front.push(polygon);
      }
      &Coplanars::SingleBuffer(node_key) => {
        node_key_by_face_key.insert(polygon.key, node_key);
        let buffer = &mut nodes[node_key].polygons;
        buffer.push(polygon);
      }
    }
  }

  pub fn push_back(
    &self,
    polygon: Polygon,
    back_key: NodeKey,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    match self {
      Coplanars::UseFrontBack => {
        node_key_by_face_key.insert(polygon.key, back_key);
        let back = &mut nodes[back_key].polygons;
        back.push(polygon);
      }
      &Coplanars::SingleBuffer(node_key) => {
        node_key_by_face_key.insert(polygon.key, node_key);
        let buffer = &mut nodes[node_key].polygons;
        buffer.push(polygon);
      }
    }
  }
}

fn triangulate_polygon<'a>(
  vertices: SmallVec<[Vertex; 4]>,
  mesh: &'a mut LinkedMesh,
) -> impl Iterator<Item = Polygon> + 'a {
  (2..vertices.len()).map(move |i| {
    let face_vertices = [
      vertices[0].clone(),
      vertices[i - 1].clone(),
      vertices[i].clone(),
    ];
    let face_key = mesh.add_face(
      [
        face_vertices[0].key,
        face_vertices[1].key,
        face_vertices[2].key,
      ],
      [None; 3],
      [false; 3],
    );
    Polygon::new(face_vertices, None, face_key)
  })
}

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
    polygon: Polygon,
    coplanars: Coplanars,
    front_key: NodeKey,
    back_key: NodeKey,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    log::info!("Splitting polygon: {:?}", polygon.key);

    let mut polygon_type = PolygonClass::Coplanar;
    let mut types = [PolygonClass::Coplanar; 3];
    for (vtx_ix, vertex) in polygon.vertices.iter().enumerate() {
      let t = self.normal.dot(&vertex.pos) - self.w;
      let polygon_class = if t < -EPSLION {
        PolygonClass::Back
      } else if t > EPSLION {
        PolygonClass::Front
      } else {
        PolygonClass::Coplanar
      };

      polygon_type = polygon_type | polygon_class;
      types[vtx_ix] = polygon_class;
    }

    // Put the polygon in the correct list, splitting it when necessary.
    match polygon_type {
      PolygonClass::Coplanar => {
        if self.normal.dot(&polygon.plane.normal) > 0. {
          coplanars.push_front(polygon, front_key, nodes, node_key_by_face_key);
        } else {
          coplanars.push_back(polygon, back_key, nodes, node_key_by_face_key);
        }
      }
      PolygonClass::Front => {
        node_key_by_face_key.insert(polygon.key, front_key);
        nodes[front_key].polygons.push(polygon);
      }
      PolygonClass::Back => {
        node_key_by_face_key.insert(polygon.key, back_key);
        nodes[back_key].polygons.push(polygon);
      }
      PolygonClass::Spanning => {
        let mut f = SmallVec::<[_; 4]>::new();
        let mut b = SmallVec::<[_; 4]>::new();

        mesh.remove_face(polygon.key);
        let split_faces = get_split_face_scratch();

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
            b.push(vi.clone());
          }
          if (ti | tj) == PolygonClass::Spanning {
            let t = (self.w - self.normal.dot(&vi.pos)) / self.normal.dot(&(vj.pos - vi.pos));
            let mut middle_vtx = vi.interpolate(vj, t);

            middle_vtx.key = if let Some(edge_key) = mesh.get_edge_key([vi.key, vj.key]) {
              let vtx_key = mesh.split_edge_cb(
                edge_key,
                EdgeSplitPos {
                  pos: t,
                  start_vtx_key: vi.key,
                },
                DisplacementNormalMethod::Interpolate,
                |old_face_key, new_face_keys| split_faces.push((old_face_key, new_face_keys)),
              );
              middle_vtx.pos = mesh.vertices[vtx_key].position;
              vtx_key
            } else {
              // The face we're splitting is the only one that uses this edge, we can just
              // add the new vertex to the mesh
              mesh.vertices.insert(linked_mesh::Vertex {
                position: middle_vtx.pos,
                shading_normal: None,
                displacement_normal: None,
                edges: Vec::new(),
              })
            };

            f.push(middle_vtx.clone());
            b.push(middle_vtx);
          }
        }

        if f.len() >= 3 {
          nodes[front_key]
            .polygons
            .extend(triangulate_polygon(f, mesh).map(|polygon| {
              log::info!("{:?} -> {:?}", polygon.key, front_key);
              node_key_by_face_key.insert(polygon.key, front_key);
              polygon
            }));
        }
        if b.len() >= 3 {
          nodes[back_key]
            .polygons
            .extend(triangulate_polygon(b, mesh).map(|polygon| {
              log::info!("{:?} -> {:?}", polygon.key, back_key);
              node_key_by_face_key.insert(polygon.key, back_key);
              polygon
            }));
        }

        log::info!("split_faces: {:?}", split_faces);
        for (old_face_key, new_face_keys) in split_faces.drain(..) {
          let node_key = node_key_by_face_key
            .remove(&old_face_key)
            .unwrap_or_else(|| panic!("Couldn't find node key for face key {old_face_key:?}"));
          log::info!("Splitting into node {:?}", node_key);
          let node = nodes
            .get_mut(node_key)
            .unwrap_or_else(|| panic!("Couldn't find node with key={node_key:?}"));
          let old_poly_ix = node
            .polygons
            .iter()
            .position(|poly| poly.key == old_face_key)
            .unwrap_or_else(|| {
              panic!(
                "Couldn't find polygon with key={old_face_key:?} in node with key={node_key:?}: \
                 \n{:?}",
                node.polygons
              )
            });
          let old_poly = node.polygons.swap_remove(old_poly_ix);
          node_key_by_face_key.remove(&old_poly.key);

          let new_faces = [&mesh.faces[new_face_keys[0]], &mesh.faces[new_face_keys[1]]];
          // TODO: Make sure winding order is correct here
          let [new_poly_0, new_poly_1] = [
            Polygon::new(
              [
                Vertex {
                  key: new_faces[0].vertices[0],
                  pos: mesh.vertices[new_faces[0].vertices[0]].position,
                },
                Vertex {
                  key: new_faces[0].vertices[1],
                  pos: mesh.vertices[new_faces[0].vertices[1]].position,
                },
                Vertex {
                  key: new_faces[0].vertices[2],
                  pos: mesh.vertices[new_faces[0].vertices[2]].position,
                },
              ],
              Some(old_poly.plane.clone()),
              new_face_keys[0],
            ),
            Polygon::new(
              [
                Vertex {
                  key: new_faces[1].vertices[0],
                  pos: mesh.vertices[new_faces[1].vertices[0]].position,
                },
                Vertex {
                  key: new_faces[1].vertices[1],
                  pos: mesh.vertices[new_faces[1].vertices[1]].position,
                },
                Vertex {
                  key: new_faces[1].vertices[2],
                  pos: mesh.vertices[new_faces[1].vertices[2]].position,
                },
              ],
              Some(old_poly.plane),
              new_face_keys[1],
            ),
          ];

          node.polygons.push(new_poly_0);
          node.polygons.push(new_poly_1);

          node_key_by_face_key.insert(new_face_keys[0], node_key);
          node_key_by_face_key.insert(new_face_keys[1], node_key);
        }
      }
    }
  }
}

#[derive(Clone, Debug)]
pub struct Vertex {
  pub pos: Vec3,
  pub key: VertexKey,
}

impl Vertex {
  fn interpolate(&self, vj: &Vertex, t: f32) -> Self {
    Vertex {
      pos: self.pos.lerp(&vj.pos, t),
      key: VertexKey::null(),
    }
  }
}

#[derive(Debug)]
pub struct Polygon {
  pub vertices: [Vertex; 3],
  pub plane: Plane,
  pub key: FaceKey,
}

impl Polygon {
  pub fn new(vertices: [Vertex; 3], plane: Option<Plane>, key: FaceKey) -> Self {
    assert!(vertices.len() >= 3);
    let plane = plane
      .unwrap_or_else(|| Plane::from_points(vertices[0].pos, vertices[1].pos, vertices[2].pos));
    Self {
      vertices,
      plane,
      key,
    }
  }

  pub fn flip(&mut self) {
    self.vertices.reverse();
    self.plane.flip();
  }
}

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
      std::mem::swap(&mut this.front, &mut this.back);
      (this.front, this.back)
    };

    if let Some(front_key) = front {
      Node::invert(front_key, nodes);
    }
    if let Some(back_key) = back {
      Node::invert(back_key, nodes);
    }
  }

  // TODO: This should accept a temp node key as well
  pub fn clip_polygons(
    self_key: NodeKey,
    polygons: Vec<Polygon>,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) -> Vec<Polygon> {
    log::info!("clip_polygons");
    let (plane, front_key, back_key) = {
      let this = &mut nodes[self_key];
      let Some(plane) = &this.plane else {
        return polygons;
      };
      (plane.clone(), this.front, this.back)
    };

    // create temporary nodes to hold the new front and back polys
    let temp_front_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    let temp_back_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });

    for polygon in polygons {
      plane.split_polygon(
        polygon,
        Coplanars::UseFrontBack,
        temp_front_key,
        temp_back_key,
        mesh,
        nodes,
        node_key_by_face_key,
      );
    }

    let mut front = nodes.remove(temp_front_key).unwrap().polygons;
    let mut back = nodes.remove(temp_back_key).unwrap().polygons;

    if let Some(front_key) = front_key {
      front = Node::clip_polygons(front_key, front, mesh, nodes, node_key_by_face_key);
    }
    if let Some(back_key) = back_key {
      back = Node::clip_polygons(back_key, back, mesh, nodes, node_key_by_face_key);
    } else {
      back = Vec::new();
    }

    front.extend(back);
    front
  }

  // Recursively remove all polygons in `polygons` that are inside this BSP tree.
  pub fn clip_to(
    self_key: NodeKey,
    bsp_key: NodeKey,
    self_mesh: &mut LinkedMesh,
    self_nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    let this_polys = {
      let this = &mut self_nodes[self_key];
      std::mem::take(&mut this.polygons)
    };
    for poly in &this_polys {
      node_key_by_face_key.remove(&poly.key);
    }
    let new_this_polygons = Node::clip_polygons(
      self_key,
      this_polys,
      self_mesh,
      self_nodes,
      node_key_by_face_key,
    );

    let (front, back) = {
      let this = &mut self_nodes[self_key];
      for poly in &new_this_polygons {
        node_key_by_face_key.insert(poly.key, self_key);
      }
      this.polygons = new_this_polygons;
      (this.front, this.back)
    };

    if let Some(front_key) = front {
      Node::clip_to(
        front_key,
        bsp_key,
        self_mesh,
        self_nodes,
        node_key_by_face_key,
      );
    }
    if let Some(back_key) = back {
      Node::clip_to(
        back_key,
        bsp_key,
        self_mesh,
        self_nodes,
        node_key_by_face_key,
      );
    }
  }

  fn into_polygons_inner(self_key: NodeKey, nodes: &mut NodeMap) -> Vec<Polygon> {
    let (mut polygons, front, back) = {
      let this = &mut nodes[self_key];
      (std::mem::take(&mut this.polygons), this.front, this.back)
    };

    if let Some(front_key) = front {
      polygons.extend(Node::into_polygons_inner(front_key, nodes));
    }
    if let Some(back_key) = back {
      polygons.extend(Node::into_polygons_inner(back_key, nodes));
    }

    polygons
  }

  /// Consumes the BSP tree and returns a list of all polygons within it.
  fn into_polygons(self_key: NodeKey, mut nodes: NodeMap) -> Vec<Polygon> {
    Self::into_polygons_inner(self_key, &mut nodes)
  }

  pub fn build(
    polygons: Vec<Polygon>,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) -> NodeKey {
    log::info!("build");
    let dummy_node_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons,
    });
    Self::build_from_temp_node(dummy_node_key, mesh, nodes, node_key_by_face_key)
  }

  /// Build a BSP tree out of `polygons`. Each set of polygons is partitioned
  /// using the first polygon (no heuristic is used to pick a good split).
  pub fn build_from_temp_node(
    dummy_node_key: NodeKey,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) -> NodeKey {
    log::info!("build_from_temp_node");
    if nodes[dummy_node_key].polygons.is_empty() {
      panic!("No polygons in temp node");
    }
    let plane = nodes[dummy_node_key].polygons[0].plane.clone();

    let temp_front_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    let temp_back_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    let self_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });

    while let Some(polygon) = nodes[dummy_node_key].polygons.pop() {
      plane.split_polygon(
        polygon,
        Coplanars::SingleBuffer(self_key),
        temp_front_key,
        temp_back_key,
        mesh,
        nodes,
        node_key_by_face_key,
      );
    }

    // for poly in &mut nodes[self_key].polygons {
    //   node_key_by_face_key.insert(poly.key, self_key);
    // }

    let front = if nodes[temp_front_key].polygons.is_empty() {
      nodes.remove(temp_front_key);
      None
    } else {
      Some(Self::build_from_temp_node(
        temp_front_key,
        mesh,
        nodes,
        node_key_by_face_key,
      ))
    };
    let back = if nodes[temp_back_key].polygons.is_empty() {
      nodes.remove(temp_back_key);
      None
    } else {
      Some(Self::build_from_temp_node(
        temp_back_key,
        mesh,
        nodes,
        node_key_by_face_key,
      ))
    };

    {
      let this = &mut nodes[self_key];
      this.plane = Some(plane);
      this.front = front;
      this.back = back;
    }

    self_key
  }

  pub fn add_polygons(
    self_key: NodeKey,
    mut polygons: Vec<Polygon>,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    log::info!("add_polygons");

    // Add a dummy node to own the polygons so that we can handle pending polygons
    // getting split
    let dummy_node_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    for poly in &mut polygons {
      node_key_by_face_key.insert(poly.key, dummy_node_key);
    }
    log::info!(
      "poly keys: {:?}",
      polygons.iter().map(|poly| poly.key).collect::<Vec<_>>()
    );
    nodes[dummy_node_key].polygons = polygons;

    log::info!("Dummy node key: {:?}", dummy_node_key);

    Self::add_polygons_from_temp_node(self_key, dummy_node_key, mesh, nodes, node_key_by_face_key);
  }

  pub fn add_polygons_from_temp_node(
    self_key: NodeKey,
    dummy_node_key: NodeKey,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    assert!(self_key != dummy_node_key);
    log::info!("add_polygons_from_temp_node");

    let temp_front_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    let temp_back_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });

    let (front_key, back_key) = {
      let plane = nodes[self_key].plane.as_ref().unwrap().clone();
      log::info!("START");
      log::info!(
        "dummy_node_key={:?}, self_key={:?}",
        dummy_node_key,
        self_key
      );
      while let Some(polygon) = nodes[dummy_node_key].polygons.pop() {
        plane.split_polygon(
          polygon,
          Coplanars::SingleBuffer(self_key),
          temp_front_key,
          temp_back_key,
          mesh,
          nodes,
          node_key_by_face_key,
        );
      }
      log::info!("END");

      nodes.remove(dummy_node_key);

      let this = &mut nodes[self_key];
      (this.front, this.back)
    };

    if nodes[temp_front_key].polygons.is_empty() {
      nodes.remove(temp_front_key);
    } else {
      match front_key {
        Some(front_key) => Node::add_polygons_from_temp_node(
          front_key,
          temp_front_key,
          mesh,
          nodes,
          node_key_by_face_key,
        ),
        None => {
          let new_front =
            Self::build_from_temp_node(temp_front_key, mesh, nodes, node_key_by_face_key);
          nodes[self_key].front = Some(new_front);
        }
      }
    }
    if nodes[temp_back_key].polygons.is_empty() {
      nodes.remove(temp_back_key);
    } else {
      match back_key {
        Some(back_key) => Node::add_polygons_from_temp_node(
          back_key,
          temp_back_key,
          mesh,
          nodes,
          node_key_by_face_key,
        ),
        None => {
          let new_back =
            Self::build_from_temp_node(temp_back_key, mesh, nodes, node_key_by_face_key);
          nodes[self_key].back = Some(new_back);
        }
      }
    }
  }
}

fn add_foreign_polys_to_linked_mesh(polys: &mut Vec<Polygon>, mesh: &mut LinkedMesh) {
  let mut new_vtx_key_by_old_vtx_key = FxHashMap::default();
  for face in polys {
    let mut map_vtx = |vtx: &mut Vertex| {
      if let Some(new_key) = new_vtx_key_by_old_vtx_key.get(&vtx.key) {
        vtx.key = *new_key;
      } else {
        let new_key = mesh.vertices.insert(linked_mesh::Vertex {
          position: vtx.pos,
          shading_normal: None,
          displacement_normal: None,
          edges: Vec::new(),
        });
        new_vtx_key_by_old_vtx_key.insert(vtx.key, new_key);
        vtx.key = new_key;
      }
    };

    for vtx in &mut face.vertices {
      map_vtx(vtx);
    }

    face.key = mesh.add_face(
      [
        face.vertices[0].key,
        face.vertices[1].key,
        face.vertices[2].key,
      ],
      [None; 3],
      [false; 3],
    );
    log::info!("Added face with key: {:?}", face.key);
  }
}

pub struct CSG {
  pub polygons: Vec<Polygon>,
  pub mesh: LinkedMesh,
}

impl CSG {
  pub fn new(polygons: Vec<Polygon>, mesh: LinkedMesh) -> Self {
    Self { polygons, mesh }
  }

  // pub fn into_polygons(self) -> Vec<Polygon> {
  //   self.polygons
  // }

  // pub fn iter_triangles<'a>(&'a self) -> impl Iterator<Item = Triangle> + 'a {
  //   self.polygons.iter().flat_map(|polygon| {
  //     (2..polygon.vertices.len()).map(move |i| {
  //       Triangle::new(
  //         polygon.vertices[i].pos,
  //         polygon.vertices[i - 1].pos,
  //         polygon.vertices[0].pos,
  //       )
  //     })
  //   })
  // }

  // pub fn to_linked_mesh(&self) -> LinkedMesh {
  //   LinkedMesh::from_triangles(self.iter_triangles())
  // }

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
  pub fn union(mut self, mut csg: CSG) -> CSG {
    let mut a_nodes = NodeMap::default();
    let mut a_node_key_by_face_key = FxHashMap::default();
    let a_key = Node::build(
      self.polygons,
      &mut self.mesh,
      &mut a_nodes,
      &mut a_node_key_by_face_key,
    );
    let mut a_mesh = self.mesh;

    let mut b_nodes = NodeMap::default();
    let mut b_node_key_by_face_key = FxHashMap::default();
    let b_key = Node::build(
      csg.polygons,
      &mut csg.mesh,
      &mut b_nodes,
      &mut b_node_key_by_face_key,
    );
    let mut b_mesh = csg.mesh;

    // a.clip_to(&mut b, &mut self.mesh);
    // b.clip_to(&mut a, &mut csg.mesh);
    // b.invert();
    // b.clip_to(&mut a, &mut csg.mesh);
    // b.invert();

    Node::clip_to(
      a_key,
      b_key,
      &mut a_mesh,
      &mut a_nodes,
      &mut a_node_key_by_face_key,
    );
    Node::clip_to(
      b_key,
      a_key,
      &mut b_mesh,
      &mut b_nodes,
      &mut b_node_key_by_face_key,
    );
    Node::invert(b_key, &mut b_nodes);
    Node::clip_to(
      b_key,
      a_key,
      &mut b_mesh,
      &mut b_nodes,
      &mut b_node_key_by_face_key,
    );
    Node::invert(b_key, &mut b_nodes);

    // let mut b_polygons = b.into_polygons();
    // add_polys_to_linked_mesh(&mut b_polygons, &mut self.mesh);
    // a.add_polygons(b_polygons, &mut self.mesh);
    // Self::new(a.into_polygons(), self.mesh)

    let mut b_polygons = Node::into_polygons(b_key, b_nodes);
    add_foreign_polys_to_linked_mesh(&mut b_polygons, &mut a_mesh);
    // TODO: TEMP
    for poly in &b_polygons {
      assert!(a_mesh.faces.contains_key(poly.key));
    }
    Node::add_polygons(
      a_key,
      b_polygons,
      &mut a_mesh,
      &mut a_nodes,
      &mut a_node_key_by_face_key,
    );
    Self::new(Node::into_polygons(a_key, a_nodes), a_mesh)
  }

  /// Returns a new CSG solid representing space in this solid but not in the
  /// solid `csg`. Neither this solid nor the solid `csg` are modified.
  ///
  ///     A.subtract(B)
  ///
  ///     +-------+            +-------+
  ///     |       |            |       |
  ///     |   A   |            |       |
  ///     |    +--+----+   =   |    +--+
  ///     +----+--+    |       +----+
  ///          |   B   |
  ///          |       |
  ///          +-------+
  // pub fn subtract(mut self, mut csg: CSG) -> CSG {
  //   let mut a = Node::build(self.polygons, &mut self.mesh);
  //   let mut b = Node::build(csg.polygons, &mut csg.mesh);

  //   a.invert();
  //   a.clip_to(&mut b, &mut self.mesh);
  //   b.clip_to(&mut a, &mut csg.mesh);
  //   b.invert();
  //   b.clip_to(&mut a, &mut csg.mesh);
  //   b.invert();

  //   let mut b_polygons = b.into_polygons();
  //   add_polys_to_linked_mesh(&mut b_polygons, &mut self.mesh);
  //   a.add_polygons(b_polygons, &mut self.mesh);
  //   Self::new(a.into_polygons(), self.mesh)
  // }

  /// Return a new CSG solid representing space both this solid and in the
  /// solid `csg`. Neither this solid nor the solid `csg` are modified.
  ///
  ///     A.intersect(B)
  ///
  ///     +-------+
  ///     |       |
  ///     |   A   |
  ///     |    +--+----+   =   +--+
  ///     +----+--+    |       +--+
  ///          |   B   |
  ///          |       |
  ///          +-------+
  // pub fn intersect(mut self, mut csg: CSG) -> Self {
  //   let mut a = Node::build(self.polygons, &mut self.mesh);
  //   let mut b = Node::build(csg.polygons, &mut csg.mesh);

  //   a.invert();
  //   a.clip_to(&mut b, &mut self.mesh);
  //   b.clip_to(&mut a, &mut csg.mesh);
  //   b.invert();
  //   b.clip_to(&mut a, &mut csg.mesh);
  //   b.invert();

  //   let mut b_polygons = b.into_polygons();
  //   add_polys_to_linked_mesh(&mut b_polygons, &mut self.mesh);
  //   a.add_polygons(b_polygons, &mut self.mesh);
  //   Self::new(a.into_polygons(), self.mesh)
  // }

  // /// Inverts the CSG in place, switching solid and empty space.
  // pub fn inverse(&mut self) {
  //   for polygon in &mut self.polygons {
  //     polygon.flip();
  //   }
  // }

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

    let mut mesh = LinkedMesh::default();
    let mut faces = Vec::new();
    for face_vertices in &polygons {
      let mut polygon_vertices = Vec::new();
      for vtx in face_vertices {
        let pos = Vec3::new(
          center[0] + radius * vtx[0] as f32,
          center[1] + radius * vtx[1] as f32,
          center[2] + radius * vtx[2] as f32,
        );
        let vtx_key = mesh.vertices.insert(linked_mesh::Vertex {
          position: pos,
          shading_normal: None,
          displacement_normal: None,
          edges: Vec::new(),
        });
        polygon_vertices.push(Vertex { pos, key: vtx_key });
      }

      faces.extend(triangulate_polygon(
        SmallVec::from_vec(polygon_vertices),
        &mut mesh,
      ));
    }

    mesh.merge_vertices_by_distance(1e-5);
    Self::new(faces, mesh)
  }
}
