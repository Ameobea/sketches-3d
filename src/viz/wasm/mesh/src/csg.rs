//! A Rust port of the CSG.js library by Evan Wallace
//! (https://github.com/evanw/csg.js/).

use std::ops::BitOr;

use arrayvec::ArrayVec;
use common::uninit;
use fxhash::FxHashMap;
use slotmap::Key;

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
  vertices: ArrayVec<Vertex, 4>,
  mesh: &'a mut LinkedMesh,
) -> impl Iterator<Item = Polygon> + 'a {
  (2..vertices.len()).map(move |i| {
    let face_vertices = [vertices[0], vertices[i - 1], vertices[i]];
    let face_key = mesh.add_face(
      [
        face_vertices[0].key,
        face_vertices[1].key,
        face_vertices[2].key,
      ],
      [None; 3],
      [false; 3],
    );
    Polygon::new(face_vertices, None, face_key, false, mesh)
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

const TEMP_NODE_KEY_0: NodeKey = unsafe { std::mem::transmute((1u32, 1u32)) };
const TEMP_NODE_KEY_1: NodeKey = unsafe { std::mem::transmute((1u32, 2u32)) };

fn handle_split_faces(
  split_faces: &mut Vec<(FaceKey, [FaceKey; 2])>,
  mesh: &mut LinkedMesh,
  nodes: &mut NodeMap,
  node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
) {
  for (old_face_key, new_face_keys) in split_faces.drain(..) {
    let node_key = node_key_by_face_key
      .remove(&old_face_key)
      .unwrap_or_else(|| panic!("Couldn't find node key for face key {old_face_key:?}"));
    let node = nodes
      .get_mut(node_key)
      .unwrap_or_else(|| panic!("Couldn't find node with key={node_key:?}"));
    let old_poly_ix = node
      .polygons
      .iter()
      .position(|poly| poly.key == old_face_key)
      .unwrap_or_else(|| {
        panic!(
          "Couldn't find polygon with key={old_face_key:?} in node with key={node_key:?}: \n{:?}",
          node.polygons
        )
      });
    let old_poly = node.polygons.swap_remove(old_poly_ix);
    node_key_by_face_key.remove(&old_poly.key);

    let new_faces = [&mesh.faces[new_face_keys[0]], &mesh.faces[new_face_keys[1]]];
    let vtx_order = if old_poly.is_flipped {
      [2, 1, 0]
    } else {
      [0, 1, 2]
    };
    node.polygons.extend((0..=1).map(|face_ix| {
      Polygon::new(
        [
          Vertex {
            key: new_faces[face_ix].vertices[vtx_order[0]],
          },
          Vertex {
            key: new_faces[face_ix].vertices[vtx_order[1]],
          },
          Vertex {
            key: new_faces[face_ix].vertices[vtx_order[2]],
          },
        ],
        Some(old_poly.plane.clone()),
        new_face_keys[face_ix],
        old_poly.is_flipped,
        mesh,
      )
    }));

    node_key_by_face_key.insert(new_face_keys[0], node_key);
    node_key_by_face_key.insert(new_face_keys[1], node_key);
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
    plane_node_key: Option<NodeKey>,
    polygon: Polygon,
    coplanars: Coplanars,
    front_key: NodeKey,
    back_key: NodeKey,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    let mut polygon_type = PolygonClass::Coplanar;
    let mut types = [PolygonClass::Coplanar; 3];
    for (vtx_ix, vertex) in polygon.vertices.iter().enumerate() {
      let t = self.normal.dot(&vertex.pos(mesh)) - self.w;
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
        log::info!("Splitting spanning polygon: {:?}", polygon.key);

        let mut f = ArrayVec::<_, 4>::new();
        let mut b = ArrayVec::<_, 4>::new();
        let mut split_vertices = ArrayVec::<_, 2>::new();

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
            let vi_pos = vi.pos(mesh);
            let t = (self.w - self.normal.dot(&vi_pos)) / self.normal.dot(&(vj.pos(mesh) - vi_pos));

            let middle_vtx_key = if let Some(edge_key) = mesh.get_edge_key([vi.key, vj.key]) {
              mesh.split_edge_cb(
                edge_key,
                EdgeSplitPos {
                  pos: t,
                  start_vtx_key: vi.key,
                },
                DisplacementNormalMethod::Interpolate,
                |old_face_key, new_face_keys| split_faces.push((old_face_key, new_face_keys)),
              )
            } else {
              // The face we're splitting is the only one that uses this edge, we can just
              // add the new vertex to the mesh
              let position = vi.interpolate(vj, t, mesh);
              mesh.vertices.insert(linked_mesh::Vertex {
                position,
                shading_normal: None,
                displacement_normal: None,
                edges: Vec::new(),
              })
            };
            let middle_vtx = Vertex {
              key: middle_vtx_key,
            };

            f.push(middle_vtx);
            b.push(middle_vtx);
            split_vertices.push(middle_vtx);
          }
        }

        if f.len() >= 3 {
          nodes[front_key]
            .polygons
            .extend(triangulate_polygon(f, mesh).map(|polygon| {
              node_key_by_face_key.insert(polygon.key, front_key);
              polygon
            }));
        }
        if b.len() >= 3 {
          nodes[back_key]
            .polygons
            .extend(triangulate_polygon(b, mesh).map(|polygon| {
              node_key_by_face_key.insert(polygon.key, back_key);
              polygon
            }));
        }

        handle_split_faces(split_faces, mesh, nodes, node_key_by_face_key);

        if let Some(plane_node_key) = plane_node_key {
          weld_polygons(
            &split_vertices,
            plane_node_key,
            mesh,
            nodes,
            node_key_by_face_key,
          );
        }
      }
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct Vertex {
  pub key: VertexKey,
}

impl Vertex {
  fn interpolate(&self, vj: &Vertex, t: f32, mesh: &LinkedMesh) -> Vec3 {
    self.pos(mesh).lerp(&vj.pos(mesh), t)
  }

  #[inline(always)]
  pub fn pos(&self, mesh: &LinkedMesh) -> Vec3 {
    mesh.vertices[self.key].position
  }
}

// TODO: Investigate just storing this as user data on the `LinkedMesh` faces
#[derive(Debug)]
pub struct Polygon {
  pub vertices: [Vertex; 3],
  pub plane: Plane,
  pub key: FaceKey,
  pub is_flipped: bool,
}

impl Polygon {
  #[inline(always)]
  pub fn new(
    vertices: [Vertex; 3],
    plane: Option<Plane>,
    key: FaceKey,
    is_flipped: bool,
    mesh: &LinkedMesh,
  ) -> Self {
    assert!(vertices.len() >= 3);
    let plane = plane.unwrap_or_else(|| {
      Plane::from_points(
        vertices[0].pos(mesh),
        vertices[1].pos(mesh),
        vertices[2].pos(mesh),
      )
    });
    Self {
      vertices,
      plane,
      key,
      is_flipped,
    }
  }

  pub fn flip(&mut self) {
    self.vertices.reverse();
    self.plane.flip();
    self.is_flipped = !self.is_flipped;
  }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Intersection {
  NoIntersection,
  WithinCenter,
  OnEdge {
    /// 0 -> (v0, v1); 1 -> (v1, v2); 2 -> (v2, v0)
    edge_ix: u8,
    /// Interpolation factor between the two vertices that the point is on
    factor: f32,
  },
  OnVertex {
    vtx_ix: u8,
  },
}

fn cartesian_vector_to_barycentric(vert_coords: [Vec3; 3], face_vec: Vec3) -> Vec3 {
  let v0 = vert_coords[1] - vert_coords[0];
  let v1 = vert_coords[2] - vert_coords[0];
  let v2 = face_vec - vert_coords[0];

  let d00 = v0.dot(&v0);
  let d01 = v0.dot(&v1);
  let d11 = v1.dot(&v1);
  let d20 = v2.dot(&v0);
  let d21 = v2.dot(&v1);
  let denom = d00 * d11 - d01 * d01;

  let v = (d11 * d20 - d01 * d21) / denom;
  let w = (d00 * d21 - d01 * d20) / denom;
  let u = 1.0 - v - w;

  Vec3::new(u, v, w)
}

#[test]
fn barycentric_correctness() {
  let tri = [
    Vec3::new(0., 0., 0.),
    Vec3::new(1., 0., 0.),
    Vec3::new(0., 1., 0.),
  ];
  let p = Vec3::new(0.5, 0.5, 0.);
  let bary = cartesian_vector_to_barycentric(tri, p);
  assert_eq!(bary, Vec3::new(0., 0.5, 0.5));
}

#[test]
fn barycentric_on_edge() {
  let tri = [
    Vec3::new(0., 0., 0.),
    Vec3::new(1., 0., 0.),
    Vec3::new(0., 1., 0.),
  ];
  let p = Vec3::new(0., 0.5, 0.);
  let bary = cartesian_vector_to_barycentric(tri, p);
  assert_eq!(bary, Vec3::new(0.5, 0., 0.5));
}

/// Determines if a point is inside a triangle in 3D space using barycentric coordinates.
fn triangle_contains_point(vert_coords: [Vec3; 3], p: Vec3, epsilon: f32) -> Intersection {
  let barycentric = cartesian_vector_to_barycentric(vert_coords, p);

  if barycentric.x < -epsilon || barycentric.y < -epsilon || barycentric.z < -epsilon {
    return Intersection::NoIntersection;
  }

  // if any coordinate is equal to 1 (considering epsilon), the point is on a vertex
  if barycentric.x > 1. - epsilon {
    return Intersection::OnVertex { vtx_ix: 0 };
  } else if barycentric.y > 1. - epsilon {
    return Intersection::OnVertex { vtx_ix: 1 };
  } else if barycentric.z > 1. - epsilon {
    return Intersection::OnVertex { vtx_ix: 2 };
  }

  // If any coordinate is equal to 0 (considering epsilon), the point is on an edge
  if barycentric.x < epsilon {
    return Intersection::OnEdge {
      edge_ix: 1,
      factor: barycentric.z,
    };
  } else if barycentric.y < epsilon {
    return Intersection::OnEdge {
      edge_ix: 2,
      factor: barycentric.x,
    };
  } else if barycentric.z < epsilon {
    return Intersection::OnEdge {
      edge_ix: 0,
      factor: barycentric.y,
    };
  }

  // If none of the above conditions are met, the point is inside the triangle
  Intersection::WithinCenter
}

#[test]
fn contains_point_on_edge() {
  let tri = [
    Vec3::new(0., 0., 0.),
    Vec3::new(1., 0., 0.),
    Vec3::new(0., 1., 0.),
  ];
  let p = Vec3::new(0., 0.5, 0.);
  let res = triangle_contains_point(tri, p, EPSLION);
  assert_eq!(
    res,
    Intersection::OnEdge {
      edge_ix: 2,
      factor: 0.5
    }
  );

  let p = Vec3::new(0.5, 0., 0.);
  let res = triangle_contains_point(tri, p, EPSLION);
  assert_eq!(
    res,
    Intersection::OnEdge {
      edge_ix: 0,
      factor: 0.5
    }
  );
}

#[test]
fn contains_point_on_vertex() {
  let tri = [
    Vec3::new(0., 0., 0.),
    Vec3::new(1., 0., 0.),
    Vec3::new(0., 1., 0.),
  ];
  let p = Vec3::new(0., 0., 0.);
  let res = triangle_contains_point(tri, p, EPSLION);
  assert_eq!(res, Intersection::OnVertex { vtx_ix: 0 });

  let p = Vec3::new(1., 0., 0.);
  let res = triangle_contains_point(tri, p, EPSLION);
  assert_eq!(res, Intersection::OnVertex { vtx_ix: 1 });
}

fn weld_polygon_at_interior(vtx: Vertex, poly: &Polygon, mesh: &mut LinkedMesh) -> [Polygon; 3] {
  log::warn!("welding vertex {:?} onto polygon {:?}", vtx.key, poly.key);

  mesh.remove_face(poly.key);

  let vertices = [
    poly.vertices[0].key,
    poly.vertices[1].key,
    poly.vertices[2].key,
    vtx.key,
  ];
  let new_poly_vertices = [[0, 1, 3], [2, 3, 1], [2, 0, 3]];
  let order = if poly.is_flipped {
    [2, 1, 0]
  } else {
    [0, 1, 2]
  };

  let mut new_polys: [Polygon; 3] = uninit();
  for i in 0..3 {
    let face_key = mesh.add_face(
      [
        vertices[new_poly_vertices[i][order[0]]],
        vertices[new_poly_vertices[i][order[1]]],
        vertices[new_poly_vertices[i][order[2]]],
      ],
      [None; 3],
      [false; 3],
    );
    let poly = Polygon::new(
      [
        Vertex {
          key: vertices[new_poly_vertices[i][0]],
        },
        Vertex {
          key: vertices[new_poly_vertices[i][1]],
        },
        Vertex {
          key: vertices[new_poly_vertices[i][2]],
        },
      ],
      Some(poly.plane.clone()),
      face_key,
      poly.is_flipped,
      mesh,
    );
    unsafe {
      std::ptr::write(&mut new_polys[i], poly);
    }
  }

  new_polys
}

/// `edge_pos` is the interpolation factor between the two vertices that the point is on
fn weld_polygon_on_edge(
  out_temp_node_key: NodeKey,
  poly: Polygon,
  edge_ix: u8,
  mesh: &mut LinkedMesh,
  nodes: &mut NodeMap,
  node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  edge_pos: f32,
) -> [Polygon; 2] {
  let [v0, v1] = match edge_ix {
    0 => [poly.vertices[0], poly.vertices[1]],
    1 => [poly.vertices[1], poly.vertices[2]],
    2 => [poly.vertices[2], poly.vertices[0]],
    _ => unreachable!(),
  };

  let split_faces = get_split_face_scratch();
  assert!(split_faces.is_empty());

  let edge = mesh.get_edge_key([v0.key, v1.key]).unwrap_or_else(|| {
    panic!(
      "Couldn't find edge key for vertices {:?} and {:?}",
      v0.key, v1.key
    )
  });
  mesh.split_edge_cb(
    edge,
    EdgeSplitPos {
      pos: edge_pos,
      start_vtx_key: v0.key,
    },
    DisplacementNormalMethod::Interpolate,
    |old_face_key, new_face_keys| split_faces.push((old_face_key, new_face_keys)),
  );

  // Need to move the polygon into the temporary node so that we can split it
  let poly_key = poly.key;
  node_key_by_face_key.insert(poly.key, out_temp_node_key);
  nodes[out_temp_node_key].polygons.push(poly);

  let new_poly_keys: [FaceKey; 2] = split_faces
    .iter()
    .find(|(old_key, _)| *old_key == poly_key)
    .unwrap()
    .1;

  handle_split_faces(split_faces, mesh, nodes, node_key_by_face_key);

  // take the new polygons out of the temporary node so we can try them with the second vertex if
  // needed
  let mut new_polys: [Polygon; 2] = uninit();
  for i in 0..2 {
    let new_poly = nodes[out_temp_node_key]
      .polygons
      .iter()
      .position(|poly| poly.key == new_poly_keys[i])
      .map(|ix| nodes[out_temp_node_key].polygons.swap_remove(ix))
      .unwrap_or_else(|| {
        panic!(
          "Couldn't find polygon with key={:?} in node with key={out_temp_node_key:?}",
          new_poly_keys[i]
        )
      });
    node_key_by_face_key.insert(new_poly.key, out_temp_node_key);
    unsafe {
      std::ptr::write(&mut new_polys[i], new_poly);
    }
  }

  new_polys
}

/// Checks if `poly` contains `vtx`.  If it does, the polygon is split into three
/// polygons and the new polygons are returned.  If it doesn't, `None` is returned.
fn maybe_weld_polygon(
  vtx: Vertex,
  out_tmp_key: NodeKey,
  poly: Polygon,
  // TODO: these three should live in a ctx struct
  mesh: &mut LinkedMesh,
  nodes: &mut NodeMap,
  node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
) -> ArrayVec<Polygon, 3> {
  let vert_coords = [
    poly.vertices[0].pos(mesh),
    poly.vertices[1].pos(mesh),
    poly.vertices[2].pos(mesh),
  ];
  let res = triangle_contains_point(vert_coords, vtx.pos(mesh), EPSLION);
  match res {
    Intersection::NoIntersection => ArrayVec::from_iter(std::iter::once(poly)),
    Intersection::WithinCenter => weld_polygon_at_interior(vtx, &poly, mesh).into(),
    Intersection::OnEdge { edge_ix, factor } => {
      log::warn!("EDGE WELDING: poly face key: {:?}", poly.key);
      let split_polys = weld_polygon_on_edge(
        out_tmp_key,
        poly,
        edge_ix,
        mesh,
        nodes,
        node_key_by_face_key,
        factor,
      );
      ArrayVec::from_iter(split_polys.into_iter())
    }
    Intersection::OnVertex { vtx_ix: _ } => {
      // I guess we ignore for now since vertices are merged at the end anyway...
      ArrayVec::from_iter(std::iter::once(poly))
    }
  }
}

fn weld_polygons(
  split_vertices: &[Vertex],
  plane_node_key: NodeKey,
  mesh: &mut LinkedMesh,
  nodes: &mut NodeMap,
  node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
) {
  if split_vertices.is_empty() {
    return;
  }

  assert!(
    split_vertices.len() <= 2,
    "should only have a max of 2 vertices intersecting a triangle; weird co-incident or fully \
     contained tri?"
  );
  let old_poly_count = {
    let plane_node = &mut nodes[plane_node_key];
    log::info!(
      "polys to consider for welding: {}",
      plane_node.polygons.len()
    );
    if plane_node.polygons.is_empty() {
      return;
    }
    plane_node.polygons.len()
  };

  // Splitting edges during this process can cause arbitrary polygons in arbitrary nodes to be
  // split, so all the in-flight/temp polygons have to live in nodes in order to keep things valid
  // during this whole process.
  let out_tmp_key = TEMP_NODE_KEY_0;
  let intermediate_tmp_key = TEMP_NODE_KEY_1;

  while let Some(poly) = nodes[plane_node_key].polygons.pop() {
    let poly_key = poly.key;

    let new_polys = maybe_weld_polygon(
      split_vertices[0],
      out_tmp_key,
      poly,
      mesh,
      nodes,
      node_key_by_face_key,
    );

    if split_vertices.len() < 2 {
      for poly in &new_polys {
        node_key_by_face_key.insert(poly.key, out_tmp_key);
      }
      nodes[out_tmp_key].polygons.extend(new_polys);
      continue;
    }

    for poly in &new_polys {
      node_key_by_face_key.insert(poly.key, intermediate_tmp_key);
    }
    nodes[intermediate_tmp_key].polygons.extend(new_polys);

    // for each new poly, we check if it contains the second vertex and split/weld it as well
    // if it does
    while let Some(poly) = nodes[intermediate_tmp_key].polygons.pop() {
      let new_polys = maybe_weld_polygon(
        split_vertices[1],
        out_tmp_key,
        poly,
        mesh,
        nodes,
        node_key_by_face_key,
      );

      for poly in &new_polys {
        node_key_by_face_key.insert(poly.key, out_tmp_key);
      }
      nodes[out_tmp_key].polygons.extend(new_polys);
    }

    node_key_by_face_key.remove(&poly_key);
  }

  assert!(nodes[intermediate_tmp_key].polygons.is_empty());

  assert!(nodes[plane_node_key].polygons.is_empty());
  let out_tmp_polys_ptr = &mut nodes[out_tmp_key].polygons as *mut Vec<Polygon>;
  std::mem::swap(&mut nodes[plane_node_key].polygons, unsafe {
    &mut *out_tmp_polys_ptr
  });
  for poly in &nodes[plane_node_key].polygons {
    node_key_by_face_key.insert(poly.key, plane_node_key);
  }

  let new_poly_count = nodes[plane_node_key].polygons.len();
  if old_poly_count != new_poly_count {
    log::info!("welded {old_poly_count} polygons into {new_poly_count} polygons");
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

  pub fn clip_polygons(
    self_key: NodeKey,
    from_key: NodeKey,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) -> Vec<Polygon> {
    let (plane, front_key, back_key) = {
      let this = &mut nodes[self_key];
      let Some(plane) = &this.plane else {
        return std::mem::take(&mut this.polygons);
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

    while let Some(polygon) = nodes[from_key].polygons.pop() {
      plane.split_polygon(
        Some(self_key),
        polygon,
        Coplanars::UseFrontBack,
        temp_front_key,
        temp_back_key,
        mesh,
        nodes,
        node_key_by_face_key,
      );
    }

    let mut front;
    let mut back = Vec::new();

    if let Some(front_key) = front_key {
      front = Node::clip_polygons(front_key, temp_front_key, mesh, nodes, node_key_by_face_key);
    } else {
      front = std::mem::take(&mut nodes[temp_front_key].polygons);
    }
    if let Some(back_key) = back_key {
      back = Node::clip_polygons(back_key, temp_back_key, mesh, nodes, node_key_by_face_key);
    } else {
      if !back.is_empty() {
        panic!("Dropping {} polygons", back.len());
      }
      back = Vec::new();
    }

    nodes.remove(temp_front_key);
    nodes.remove(temp_back_key);

    front.extend(back);
    front
  }

  // Recursively remove all polygons in `polygons` that are inside this BSP tree.
  pub fn clip_to(
    self_key: NodeKey,
    bsp_key: NodeKey,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) {
    let new_this_polygons =
      Node::clip_polygons(bsp_key, self_key, mesh, nodes, node_key_by_face_key);

    let (front, back) = {
      let this = &mut nodes[self_key];
      for poly in &new_this_polygons {
        node_key_by_face_key.insert(poly.key, self_key);
      }
      this.polygons = new_this_polygons;
      (this.front, this.back)
    };

    if let Some(front_key) = front {
      Node::clip_to(front_key, bsp_key, mesh, nodes, node_key_by_face_key);
    }
    if let Some(back_key) = back {
      Node::clip_to(back_key, bsp_key, mesh, nodes, node_key_by_face_key);
    }
  }

  /// Consumes the BSP tree and returns a list of all polygons within it.
  fn into_polygons(self_key: NodeKey, nodes: &mut NodeMap) -> Vec<Polygon> {
    let (mut polygons, front, back) = {
      let this = &mut nodes[self_key];
      (std::mem::take(&mut this.polygons), this.front, this.back)
    };

    if let Some(front_key) = front {
      polygons.extend(Node::into_polygons(front_key, nodes));
    }
    if let Some(back_key) = back {
      polygons.extend(Node::into_polygons(back_key, nodes));
    }

    polygons
  }

  pub fn build(
    polygons: Vec<Polygon>,
    mesh: &mut LinkedMesh,
    nodes: &mut NodeMap,
    node_key_by_face_key: &mut FxHashMap<FaceKey, NodeKey>,
  ) -> NodeKey {
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
        None,
        polygon,
        Coplanars::SingleBuffer(self_key),
        temp_front_key,
        temp_back_key,
        mesh,
        nodes,
        node_key_by_face_key,
      );
    }

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
    nodes[dummy_node_key].polygons = polygons;

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
      while let Some(polygon) = nodes[dummy_node_key].polygons.pop() {
        plane.split_polygon(
          None,
          polygon,
          Coplanars::SingleBuffer(self_key),
          temp_front_key,
          temp_back_key,
          mesh,
          nodes,
          node_key_by_face_key,
        );
      }

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

pub struct CSG {
  pub polygons: Vec<Polygon>,
  pub mesh: LinkedMesh,
}

impl CSG {
  pub fn new(polygons: Vec<Polygon>, mesh: LinkedMesh) -> Self {
    Self { polygons, mesh }
  }

  fn merge_other(mesh: &mut LinkedMesh, other: LinkedMesh) -> Vec<Polygon> {
    let mut our_vtx_key_by_other_vtx_key = FxHashMap::default();
    for (vtx_key, vtx) in other.vertices.iter() {
      let new_key = mesh.vertices.insert(linked_mesh::Vertex {
        position: vtx.position,
        shading_normal: None,
        displacement_normal: None,
        edges: Vec::new(),
      });
      our_vtx_key_by_other_vtx_key.insert(vtx_key, new_key);
    }
    let csg_polys = other
      .faces
      .values()
      .map(|face| {
        let vertices = [
          our_vtx_key_by_other_vtx_key[&face.vertices[0]],
          our_vtx_key_by_other_vtx_key[&face.vertices[1]],
          our_vtx_key_by_other_vtx_key[&face.vertices[2]],
        ];
        let face_key = mesh.add_face(vertices, [None; 3], [false; 3]);
        let face_vertices = [
          Vertex { key: vertices[0] },
          Vertex { key: vertices[1] },
          Vertex { key: vertices[2] },
        ];
        Polygon::new(face_vertices, None, face_key, false, &mesh)
      })
      .collect::<Vec<_>>();

    csg_polys
  }

  /// Inits a node map with some special hard-coded keys that are used as temporary buffers to avoid
  /// allocating
  fn init_nodes() -> NodeMap {
    let mut nodes = NodeMap::default();
    let tmp0 = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    assert_eq!(tmp0, TEMP_NODE_KEY_0);
    let tmp1 = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    assert_eq!(tmp1, TEMP_NODE_KEY_1);
    nodes
  }

  fn init(
    self,
    other: LinkedMesh,
  ) -> (
    LinkedMesh,
    NodeMap,
    FxHashMap<FaceKey, NodeKey>,
    NodeKey,
    NodeKey,
  ) {
    let mut nodes = Self::init_nodes();
    let mut node_key_by_face_key = FxHashMap::default();
    let mut mesh = self.mesh;
    let a_key = Node::build(
      self.polygons,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );

    let csg_polygons = Self::merge_other(&mut mesh, other);

    let b_key = Node::build(
      csg_polygons,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );

    (mesh, nodes, node_key_by_face_key, a_key, b_key)
  }

  fn extract_mesh(mesh: LinkedMesh, mut nodes: NodeMap, a_key: NodeKey) -> LinkedMesh {
    let mut new_mesh = LinkedMesh::default();
    for poly in Node::into_polygons(a_key, &mut nodes) {
      let mut face_vertices = [VertexKey::null(); 3];
      for (i, vtx) in poly.vertices.into_iter().enumerate() {
        let vtx_key = new_mesh.vertices.insert(linked_mesh::Vertex {
          position: vtx.pos(&mesh),
          shading_normal: None,
          displacement_normal: None,
          edges: Vec::new(),
        });
        face_vertices[i] = vtx_key;
      }
      new_mesh.add_face(face_vertices, [None; 3], [false; 3]);
    }
    new_mesh.merge_vertices_by_distance(1e-5);
    new_mesh
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
  pub fn union(self, other: LinkedMesh) -> LinkedMesh {
    let (mut mesh, mut nodes, mut node_key_by_face_key, a_key, b_key) = self.init(other);

    Node::clip_to(
      a_key,
      b_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    Node::clip_to(
      b_key,
      a_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    Node::invert(b_key, &mut nodes);
    Node::clip_to(
      b_key,
      a_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    Node::invert(b_key, &mut nodes);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(
      a_key,
      b_polygons,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );

    Self::extract_mesh(mesh, nodes, a_key)
  }

  /// Removes all parts of `other` that are inside of `self` and returns the result.
  pub fn clip_to_self(self, other: LinkedMesh) -> LinkedMesh {
    let (mut mesh, mut nodes, mut node_key_by_face_key, a_key, b_key) = self.init(other);

    Node::clip_to(
      b_key,
      a_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(
      a_key,
      b_polygons,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );

    Self::extract_mesh(mesh, nodes, a_key)
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
  pub fn subtract(self, other: LinkedMesh) -> LinkedMesh {
    let (mut mesh, mut nodes, mut node_key_by_face_key, a_key, b_key) = self.init(other);

    log::info!("a.invert()");
    Node::invert(a_key, &mut nodes);
    log::info!("a.clip_to(b)");
    Node::clip_to(
      a_key,
      b_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    log::info!("b.clip_to(a)");
    Node::clip_to(
      b_key,
      a_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    log::info!("b.invert()");
    Node::invert(b_key, &mut nodes);
    log::info!("b.clip_to(a)");
    Node::clip_to(
      b_key,
      a_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    log::info!("b.invert()");
    Node::invert(b_key, &mut nodes);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(
      a_key,
      b_polygons,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    Node::invert(a_key, &mut nodes);

    Self::extract_mesh(mesh, nodes, a_key)
  }

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
  pub fn intersect(self, csg: LinkedMesh) -> LinkedMesh {
    let (mut mesh, mut nodes, mut node_key_by_face_key, a_key, b_key) = self.init(csg);

    Node::invert(a_key, &mut nodes);
    Node::clip_to(
      b_key,
      a_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    Node::invert(b_key, &mut nodes);
    Node::clip_to(
      a_key,
      b_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    Node::clip_to(
      b_key,
      a_key,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(
      a_key,
      b_polygons,
      &mut mesh,
      &mut nodes,
      &mut node_key_by_face_key,
    );
    Node::invert(a_key, &mut nodes);

    Self::extract_mesh(mesh, nodes, a_key)
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

    let mut mesh = LinkedMesh::default();
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
        polygon_vertices.push(Vertex { key: vtx_key });
      }

      for _ in triangulate_polygon(ArrayVec::from_iter(polygon_vertices), &mut mesh) {
        // pass
      }
    }

    mesh.merge_vertices_by_distance(1e-5);
    let mut faces = Vec::with_capacity(mesh.faces.len());
    for (face_key, face) in mesh.faces.iter() {
      let vertices = [
        Vertex {
          key: face.vertices[0],
        },
        Vertex {
          key: face.vertices[1],
        },
        Vertex {
          key: face.vertices[2],
        },
      ];
      faces.push(Polygon::new(vertices, None, face_key, false, &mesh));
    }
    Self::new(faces, mesh)
  }
}

impl From<LinkedMesh> for CSG {
  fn from(mesh: LinkedMesh) -> Self {
    let polygons = mesh
      .faces
      .iter()
      .map(|(face_key, face)| {
        let vertices = [
          Vertex {
            key: face.vertices[0],
          },
          Vertex {
            key: face.vertices[1],
          },
          Vertex {
            key: face.vertices[2],
          },
        ];
        Polygon::new(vertices, None, face_key, false, &mesh)
      })
      .collect();
    Self { polygons, mesh }
  }
}
