//! A Rust port of the CSG.js library by Evan Wallace
//! (https://github.com/evanw/csg.js/).

use std::ops::BitOr;

use arrayvec::ArrayVec;
use bitvec::vec::BitVec;
use common::uninit;
use fxhash::{FxHashMap, FxHashSet};
use slotmap::Key;

use crate::{
  linked_mesh::{self, DisplacementNormalMethod, EdgeSplitPos, FaceKey, Vec3, VertexKey},
  LinkedMesh,
};

const EPSILON: f32 = 1e-5;

slotmap::new_key_type! {
  pub struct NodeKey;
}

pub type NodeMap = slotmap::SlotMap<NodeKey, Node>;

static mut SPLIT_FACE_CACHE: *mut Vec<((FaceKey, FaceData), [FaceKey; 2])> = std::ptr::null_mut();

fn init_split_face_scratch() {
  unsafe {
    SPLIT_FACE_CACHE = Box::into_raw(Box::new(Vec::new()));
  }
}

fn get_split_face_scratch() -> &'static mut Vec<((FaceKey, FaceData), [FaceKey; 2])> {
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

pub struct FaceData {
  pub plane: Plane,
  pub node_key: NodeKey,
}

impl Default for FaceData {
  fn default() -> Self {
    Self {
      plane: Plane {
        normal: Vec3::zeros(),
        w: 0.,
      },
      node_key: NodeKey::null(),
    }
  }
}

impl Coplanars {
  pub fn push_front(
    &self,
    polygon: Polygon,
    front_key: NodeKey,
    nodes: &mut NodeMap,
    mesh: &mut LinkedMesh<FaceData>,
  ) {
    match self {
      Coplanars::UseFrontBack => {
        polygon.set_node_key(front_key, mesh);
        let front = &mut nodes[front_key].polygons;
        front.push(polygon);
      }
      &Coplanars::SingleBuffer(node_key) => {
        polygon.set_node_key(node_key, mesh);
        let buffer = &mut nodes[node_key].polygons;
        buffer.push(polygon);
      }
    }
  }

  pub fn push_back(
    &self,
    poly: Polygon,
    back_key: NodeKey,
    nodes: &mut NodeMap,
    mesh: &mut LinkedMesh<FaceData>,
  ) {
    match self {
      Coplanars::UseFrontBack => {
        poly.set_node_key(back_key, mesh);
        let back = &mut nodes[back_key].polygons;
        back.push(poly);
      }
      &Coplanars::SingleBuffer(node_key) => {
        poly.set_node_key(node_key, mesh);
        let buffer = &mut nodes[node_key].polygons;
        buffer.push(poly);
      }
    }
  }
}

fn triangulate_polygon<'a>(
  vertices: ArrayVec<Vertex, 4>,
  mesh: &'a mut LinkedMesh<FaceData>,
  plane: Plane,
  node_key: NodeKey,
) -> impl Iterator<Item = Polygon> + 'a {
  (2..vertices.len()).map(move |i| {
    let face_vertices = [vertices[0], vertices[i - 1], vertices[i]];
    let face_key = mesh.add_face(
      [
        face_vertices[0].key,
        face_vertices[1].key,
        face_vertices[2].key,
      ],
      FaceData {
        plane: plane.clone(),
        node_key,
      },
    );
    Polygon::new(face_key)
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
  split_faces: &mut Vec<((FaceKey, FaceData), [FaceKey; 2])>,
  mesh: &mut LinkedMesh<FaceData>,
  nodes: &mut NodeMap,
) {
  for ((old_face_key, old_face_data), new_face_keys) in split_faces.drain(..) {
    let node_key = old_face_data.node_key;
    let node = nodes.get_mut(node_key).unwrap_or_else(|| {
      panic!(
        "Couldn't find node with key={node_key:?} referenced by face with key={old_face_key:?}"
      )
    });
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
    node.polygons.swap_remove(old_poly_ix);

    node.polygons.extend((0..=1).filter_map(|face_ix| {
      let new_face_key = new_face_keys[face_ix];
      if mesh.faces[new_face_keys[face_ix]].is_degenerate(&mesh.vertices) {
        mesh.remove_face(new_face_key);
        return None;
      }

      let poly = Polygon::new(new_face_key);
      let user_data = poly.user_data_mut(mesh);
      user_data.plane = old_face_data.plane.clone();
      user_data.node_key = node_key;
      Some(poly)
    }));
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

  // Finds an arbitrary point on the plane
  fn point_on_plane(&self) -> Vec3 {
    if self.normal.x.abs() > 0.1 {
      // Avoid division by a small number
      Vec3::new(-self.w / self.normal.x, 0.0, 0.0)
    } else if self.normal.y.abs() > 0.1 {
      Vec3::new(0.0, -self.w / self.normal.y, 0.0)
    } else {
      Vec3::new(0.0, 0.0, -self.w / self.normal.z)
    }
  }

  // Compute two orthogonal vectors in the plane
  pub fn compute_basis(&self) -> (Vec3, Vec3) {
    let u = if self.normal.x.abs() > self.normal.z.abs() {
      Vec3::new(-self.normal.y, self.normal.x, 0.0).normalize()
    } else {
      Vec3::new(0.0, -self.normal.z, self.normal.y).normalize()
    };
    let v = self.normal.cross(&u).normalize();
    (u, v)
  }

  // Project a 3D point to this plane's 2D coordinates
  pub fn to_2d(&self, point: Vec3, u: &Vec3, v: &Vec3) -> (f32, f32) {
    let point_on_plane = self.point_on_plane();
    let relative_point = point - point_on_plane;
    let x = relative_point.dot(u);
    let y = relative_point.dot(v);
    (x, y)
  }

  // Reconstruct a 3D point from 2D coordinates in this plane
  pub fn to_3d(&self, x: f32, y: f32, u: &Vec3, v: &Vec3) -> Vec3 {
    let point_on_plane = self.point_on_plane();
    point_on_plane + u * x + v * y
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
    mesh: &mut LinkedMesh<FaceData>,
    nodes: &mut NodeMap,
  ) {
    let mut polygon_type = PolygonClass::Coplanar;
    let mut types = [PolygonClass::Coplanar; 3];
    for (vtx_ix, vertex) in mesh.faces[polygon.key]
      .vertices
      .iter()
      .map(|vtx_key| Vertex { key: *vtx_key })
      .enumerate()
    {
      let t = self.normal.dot(&vertex.pos(mesh)) - self.w;
      let polygon_class = if t < -EPSILON {
        PolygonClass::Back
      } else if t > EPSILON {
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
        if self.normal.dot(&polygon.plane(mesh).normal) > 0. {
          coplanars.push_front(polygon, front_key, nodes, mesh);
        } else {
          coplanars.push_back(polygon, back_key, nodes, mesh);
        }
      }
      PolygonClass::Front => {
        polygon.set_node_key(front_key, mesh);
        nodes[front_key].polygons.push(polygon);
      }
      PolygonClass::Back => {
        polygon.set_node_key(back_key, mesh);
        nodes[back_key].polygons.push(polygon);
      }
      PolygonClass::Spanning => {
        let mut f = ArrayVec::<Vertex, 4>::new();
        let mut b = ArrayVec::<Vertex, 4>::new();
        let mut split_vertices = ArrayVec::<_, 2>::new();

        let verts = [
          polygon.vtx(0, mesh),
          polygon.vtx(1, mesh),
          polygon.vtx(2, mesh),
        ];
        let old_poly_user_data = mesh.remove_face(polygon.key);
        let split_faces = get_split_face_scratch();

        for i in 0..3 {
          let j = (i + 1) % 3;
          let ti = types[i];
          let tj = types[j];
          let vi = verts[i];
          let vj = verts[j];

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
                |old_face_key, old_face_data, new_face_keys| {
                  split_faces.push(((old_face_key, old_face_data), new_face_keys))
                },
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
          nodes[front_key].polygons.extend(triangulate_polygon(
            f,
            mesh,
            old_poly_user_data.plane.clone(),
            front_key,
          ));
        }
        if b.len() >= 3 {
          nodes[back_key].polygons.extend(triangulate_polygon(
            b,
            mesh,
            old_poly_user_data.plane.clone(),
            back_key,
          ));
        }

        handle_split_faces(split_faces, mesh, nodes);

        if let Some(plane_node_key) = plane_node_key {
          // weld_polygons(&split_vertices, plane_node_key, mesh, nodes);
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
  fn interpolate<T>(&self, vj: Vertex, t: f32, mesh: &LinkedMesh<T>) -> Vec3 {
    self.pos(mesh).lerp(&vj.pos(mesh), t)
  }

  #[inline(always)]
  pub fn pos<T>(&self, mesh: &LinkedMesh<T>) -> Vec3 {
    if cfg!(feature = "unsafe_indexing") {
      unsafe { mesh.vertices.get_unchecked(self.key).position }
    } else {
      mesh.vertices[self.key].position
    }
  }
}

#[derive(Debug)]
pub struct Polygon {
  pub key: FaceKey,
}

impl Polygon {
  #[inline(always)]
  pub fn new(key: FaceKey) -> Self {
    Self { key }
  }

  pub fn user_data<'a, T>(&self, mesh: &'a LinkedMesh<T>) -> &'a T {
    if cfg!(feature = "unsafe_indexing") {
      unsafe { &mesh.faces.get_unchecked(self.key).data }
    } else {
      &mesh.faces[self.key].data
    }
  }

  pub fn user_data_mut<'a, T>(&self, mesh: &'a mut LinkedMesh<T>) -> &'a mut T {
    if cfg!(feature = "unsafe_indexing") {
      unsafe { &mut mesh.faces.get_unchecked_mut(self.key).data }
    } else {
      &mut mesh.faces[self.key].data
    }
  }

  pub fn plane<'a>(&self, mesh: &'a LinkedMesh<FaceData>) -> &'a Plane {
    &self.user_data(mesh).plane
  }

  pub fn plane_mut<'a>(&self, mesh: &'a mut LinkedMesh<FaceData>) -> &'a mut Plane {
    &mut self.user_data_mut(mesh).plane
  }

  pub fn flip(&mut self, mesh: &mut LinkedMesh<FaceData>) {
    self.plane_mut(mesh).flip();
    let verts = &mut mesh.faces[self.key].vertices;
    verts.swap(0, 2);
  }

  fn compute_plane(&self, mesh: &mut LinkedMesh<FaceData>) {
    *self.plane_mut(mesh) = Plane::from_points(
      self.vtx(0, mesh).pos(mesh),
      self.vtx(1, mesh).pos(mesh),
      self.vtx(2, mesh).pos(mesh),
    );
  }

  fn set_node_key(&self, node_key: NodeKey, mesh: &mut LinkedMesh<FaceData>) {
    self.user_data_mut(mesh).node_key = node_key;
  }

  fn vtx(&self, ix: usize, mesh: &LinkedMesh<FaceData>) -> Vertex {
    Vertex {
      key: if cfg!(feature = "unsafe_indexing") {
        unsafe { mesh.faces.get_unchecked(self.key).vertices[ix] }
      } else {
        mesh.faces[self.key].vertices[ix]
      },
    }
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

fn cartesian_vector_to_barycentric(vert_coords: &[Vec3; 3], face_vec: Vec3) -> Vec3 {
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
  let bary = cartesian_vector_to_barycentric(&tri, p);
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
  let bary = cartesian_vector_to_barycentric(&tri, p);
  assert_eq!(bary, Vec3::new(0.5, 0., 0.5));
}

/// Determines if a point is inside a triangle in 3D space using barycentric coordinates.
fn triangle_contains_point(vert_coords: &[Vec3; 3], p: Vec3, epsilon: f32) -> Intersection {
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
  let res = triangle_contains_point(&tri, p, EPSILON);
  assert_eq!(
    res,
    Intersection::OnEdge {
      edge_ix: 2,
      factor: 0.5
    }
  );

  let p = Vec3::new(0.5, 0., 0.);
  let res = triangle_contains_point(&tri, p, EPSILON);
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
  let res = triangle_contains_point(&tri, p, EPSILON);
  assert_eq!(res, Intersection::OnVertex { vtx_ix: 0 });

  let p = Vec3::new(1., 0., 0.);
  let res = triangle_contains_point(&tri, p, EPSILON);
  assert_eq!(res, Intersection::OnVertex { vtx_ix: 1 });
}

fn weld_polygon_at_interior<'a>(
  vtx: Vertex,
  poly: &Polygon,
  mesh: &'a mut LinkedMesh<FaceData>,
) -> impl Iterator<Item = Polygon> + 'a {
  let verts = &mesh.faces[poly.key].vertices;
  let verts = [
    [verts[0], verts[1], vtx.key],
    [verts[2], vtx.key, verts[1]],
    [verts[2], verts[0], vtx.key],
  ];
  let old_user_data = mesh.remove_face(poly.key);

  verts.into_iter().map(move |verts| {
    let face_key = mesh.add_face(
      verts,
      FaceData {
        plane: old_user_data.plane.clone(),
        node_key: old_user_data.node_key,
      },
    );
    Polygon::new(face_key)
  })
}

/// `edge_pos` is the interpolation factor between the two vertices that the point is on
fn weld_polygon_on_edge<'a>(
  out_temp_node_key: NodeKey,
  poly: Polygon,
  edge_ix: u8,
  mesh: &'a mut LinkedMesh<FaceData>,
  nodes: &'a mut NodeMap,
  edge_pos: f32,
) -> impl Iterator<Item = Polygon> + 'a {
  let [v0, v1] = match edge_ix {
    0 => [poly.vtx(0, mesh), poly.vtx(1, mesh)],
    1 => [poly.vtx(1, mesh), poly.vtx(2, mesh)],
    2 => [poly.vtx(2, mesh), poly.vtx(0, mesh)],
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

  // Need to move the polygon into the temporary node so that we can split it
  let poly_key = poly.key;
  poly.set_node_key(out_temp_node_key, mesh);
  nodes[out_temp_node_key].polygons.push(poly);

  mesh.split_edge_cb(
    edge,
    EdgeSplitPos {
      pos: edge_pos,
      start_vtx_key: v0.key,
    },
    DisplacementNormalMethod::Interpolate,
    |old_face_key, old_face_data, new_face_keys| {
      split_faces.push(((old_face_key, old_face_data), new_face_keys))
    },
  );

  let new_poly_keys: [FaceKey; 2] = split_faces
    .iter()
    .find(|((old_key, _old_data), _new_keys)| *old_key == poly_key)
    .unwrap()
    .1;

  handle_split_faces(split_faces, mesh, nodes);

  // take the new polygons out of the temporary node so we can try them with the second vertex if
  // needed
  (0..2).filter_map(move |i| {
    let poly = nodes[out_temp_node_key]
      .polygons
      .iter()
      .position(|poly| poly.key == new_poly_keys[i])
      .map(|ix| nodes[out_temp_node_key].polygons.swap_remove(ix))?;
    poly.set_node_key(out_temp_node_key, mesh);
    Some(poly)
  })
}

/// Checks if `poly` contains `vtx`.  If it does, the polygon is split into three
/// polygons and the new polygons are returned.  If it doesn't, `None` is returned.
fn maybe_weld_polygon(
  vtx: Vertex,
  out_tmp_key: NodeKey,
  poly: Polygon,
  mesh: &mut LinkedMesh<FaceData>,
  nodes: &mut NodeMap,
) -> ArrayVec<Polygon, 3> {
  let vert_coords = [
    poly.vtx(0, mesh).pos(mesh),
    poly.vtx(1, mesh).pos(mesh),
    poly.vtx(2, mesh).pos(mesh),
  ];

  // the epsilon value below seems to matter quite a bit.  The triangle intersection test seems
  // quite prone to floating point precision issues.
  let ixn = triangle_contains_point(&vert_coords, vtx.pos(mesh), 1e-4);
  match ixn {
    Intersection::NoIntersection => ArrayVec::from_iter(std::iter::once(poly)),
    Intersection::WithinCenter => ArrayVec::from_iter(weld_polygon_at_interior(vtx, &poly, mesh)),
    Intersection::OnEdge { edge_ix, factor } => {
      let split_polys = weld_polygon_on_edge(out_tmp_key, poly, edge_ix, mesh, nodes, factor);
      ArrayVec::from_iter(split_polys)
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
  mesh: &mut LinkedMesh<FaceData>,
  nodes: &mut NodeMap,
) {
  if split_vertices.is_empty() {
    return;
  }

  assert!(
    split_vertices.len() <= 2,
    "should only have a max of 2 vertices intersecting a triangle; weird co-incident or fully \
     contained tri?"
  );
  let plane_node = &mut nodes[plane_node_key];
  if plane_node.polygons.is_empty() {
    return;
  }

  // Splitting edges during this process can cause arbitrary polygons in arbitrary nodes to be
  // split, so all the in-flight/temp polygons have to live in nodes in order to keep things valid
  // during this whole process.
  let out_tmp_key = TEMP_NODE_KEY_0;
  let intermediate_tmp_key = TEMP_NODE_KEY_1;

  while let Some(poly) = nodes[plane_node_key].polygons.pop() {
    let new_polys = maybe_weld_polygon(split_vertices[0], out_tmp_key, poly, mesh, nodes);

    if split_vertices.len() < 2 {
      for poly in &new_polys {
        poly.set_node_key(out_tmp_key, mesh);
      }
      nodes[out_tmp_key].polygons.extend(new_polys);
      continue;
    }

    for poly in &new_polys {
      poly.set_node_key(intermediate_tmp_key, mesh);
    }
    nodes[intermediate_tmp_key].polygons.extend(new_polys);

    // for each new poly, we check if it contains the second vertex and split/weld it as well
    // if it does
    while let Some(poly) = nodes[intermediate_tmp_key].polygons.pop() {
      let new_polys = maybe_weld_polygon(split_vertices[1], out_tmp_key, poly, mesh, nodes);

      for poly in &new_polys {
        poly.set_node_key(out_tmp_key, mesh);
      }
      nodes[out_tmp_key].polygons.extend(new_polys);
    }
  }

  assert!(nodes[intermediate_tmp_key].polygons.is_empty());

  assert!(nodes[plane_node_key].polygons.is_empty());
  let out_tmp_polys_ptr = &mut nodes[out_tmp_key].polygons as *mut Vec<Polygon>;
  std::mem::swap(&mut nodes[plane_node_key].polygons, unsafe {
    &mut *out_tmp_polys_ptr
  });
  for poly in &nodes[plane_node_key].polygons {
    poly.set_node_key(plane_node_key, mesh);
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
  pub fn invert(self_key: NodeKey, nodes: &mut NodeMap, mesh: &mut LinkedMesh<FaceData>) {
    let (front, back) = {
      let this = &mut nodes[self_key];
      for polygon in &mut this.polygons {
        polygon.flip(mesh);
      }
      if let Some(plane) = &mut this.plane {
        plane.flip();
      }
      std::mem::swap(&mut this.front, &mut this.back);
      (this.front, this.back)
    };

    if let Some(front_key) = front {
      Node::invert(front_key, nodes, mesh);
    }
    if let Some(back_key) = back {
      Node::invert(back_key, nodes, mesh);
    }
  }

  pub fn clip_polygons(
    self_key: NodeKey,
    from_key: NodeKey,
    mesh: &mut LinkedMesh<FaceData>,
    nodes: &mut NodeMap,
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
      );
    }

    let mut front = Vec::new();
    let mut back = Vec::new();

    if let Some(front_key) = front_key {
      front = Node::clip_polygons(front_key, temp_front_key, mesh, nodes);
    }
    if let Some(back_key) = back_key {
      back = Node::clip_polygons(back_key, temp_back_key, mesh, nodes);
    } else {
      if !back.is_empty() {
        panic!("Dropping {} polygons", back.len());
      }
      back = Vec::new();
    }

    if front_key.is_none() {
      front = std::mem::take(&mut nodes[temp_front_key].polygons);
    }

    assert!(nodes.remove(temp_front_key).unwrap().polygons.is_empty());
    for poly in nodes.remove(temp_back_key).unwrap().polygons {
      mesh.remove_face(poly.key);
    }

    front.extend(back);
    front
  }

  // Recursively remove all polygons in `polygons` that are inside this BSP tree.
  pub fn clip_to(
    self_key: NodeKey,
    bsp_key: NodeKey,
    mesh: &mut LinkedMesh<FaceData>,
    nodes: &mut NodeMap,
  ) {
    let new_this_polygons = Node::clip_polygons(bsp_key, self_key, mesh, nodes);

    let (front, back) = {
      let this = &mut nodes[self_key];
      for poly in &new_this_polygons {
        poly.set_node_key(self_key, mesh);
      }
      this.polygons = new_this_polygons;
      (this.front, this.back)
    };

    if let Some(front_key) = front {
      Node::clip_to(front_key, bsp_key, mesh, nodes);
    }
    if let Some(back_key) = back {
      Node::clip_to(back_key, bsp_key, mesh, nodes);
    }
  }

  /// Re-triangulate all polygons in this node using Delaunay triangulation.  This helps reduce the
  /// amount of long, skinny triangles that can be created by the splitting process.
  pub fn remesh(&mut self, self_key: NodeKey, mesh: &mut LinkedMesh<FaceData>) {
    if self.polygons.is_empty() {
      return;
    }

    // TODO: if we keep the current setup of only calling this at the end of the whole process, we
    // can avoid re-pushing all of these faces back into the mesh and instead just call this when
    // draining them.

    let plane = self.polygons[0].plane(mesh).clone();
    let (u, v) = plane.compute_basis();

    let mut vtx_keys = Vec::with_capacity(self.polygons.len() * 3);
    let points_2d = self
      .polygons
      .drain(..)
      .flat_map(|poly| {
        vtx_keys.extend_from_slice(&mesh.faces[poly.key].vertices);
        let vtx_positions = mesh.faces[poly.key].vertex_positions(&mesh.vertices);
        mesh.remove_face(poly.key);
        vtx_positions.into_iter().map(|pos| {
          let (x, y) = plane.to_2d(pos, &u, &v);
          delaunator::Point {
            x: x as f64,
            y: y as f64,
          }
        })
      })
      .collect::<Vec<_>>();

    let triangles = delaunator::triangulate(&points_2d).triangles;

    assert!(triangles.len() % 3 == 0);

    self
      .polygons
      .extend(triangles.array_chunks::<3>().map(|[i0, i1, i2]| {
        let vtx_keys = [vtx_keys[*i2], vtx_keys[*i1], vtx_keys[*i0]];
        let face_key = mesh.add_face(
          vtx_keys,
          FaceData {
            plane: plane.clone(),
            node_key: self_key,
          },
        );
        let poly = Polygon::new(face_key);
        poly.set_node_key(self_key, mesh);
        poly
      }));
  }

  fn collapse_interior_vertices(&mut self, mesh: &mut LinkedMesh<FaceData>) {
    let all_vtx_keys: FxHashSet<_> = self
      .polygons
      .iter()
      .flat_map(|poly| mesh.faces[poly.key].vertices)
      .collect();
    let all_vtx_keys_vec: Vec<_> = all_vtx_keys.iter().copied().collect();

    let all_face_keys = self
      .polygons
      .iter()
      .map(|poly| poly.key)
      .collect::<FxHashSet<_>>();
    let interior_flags: FxHashMap<VertexKey, bool> = all_vtx_keys
      .iter()
      .map(|&vtx_key| (vtx_key, mesh.vertex_has_full_fan(vtx_key, &all_face_keys)))
      .collect();

    let mut removed_face_keys = FxHashSet::default();
    for vtx_key in all_vtx_keys_vec {
      // vtx might have already been deleted
      if !mesh.vertices.contains_key(vtx_key) {
        continue;
      }

      // let is_interior = mesh.vertex_has_full_fan(vtx_key, &all_face_keys);
      // if !is_interior {
      //   continue;
      // }
      if interior_flags[&vtx_key] {
        continue;
      }

      // collapse the longest edge that's fully contained within this plane
      let Some((edge_key_to_collapse, vtx_to_merge_into, _edge_len)) = mesh.vertices[vtx_key]
        .edges
        .iter()
        .filter_map(|&edge_key| {
          let edge = &mesh.edges[edge_key];
          if !edge
            .faces
            .iter()
            .all(|face_key| all_face_keys.contains(face_key))
          {
            return None;
          }

          // only merge with other interior vertices to avoid weird topological issues
          let o_vtx_key = edge.other_vtx(vtx_key);
          // TODO TEMP
          assert!(all_vtx_keys.contains(&o_vtx_key));
          if !interior_flags[&o_vtx_key] {
            // return None;
          }

          let edge_len = edge.length(&mesh.vertices);
          Some((edge_key, o_vtx_key, edge_len))
        })
        .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
      else {
        continue;
      };

      // Remove any faces that use the edge we're about to collapse.  This won't result in any holes
      // in the mesh since they will be closed as a result of the merge.
      let face_keys = mesh.edges[edge_key_to_collapse].faces.clone();
      for face_key in face_keys {
        mesh.remove_face(face_key);
        removed_face_keys.insert(face_key);
        // TODO: this should never happen...
        assert!(all_face_keys.contains(&face_key));
      }

      mesh.merge_vertices(vtx_to_merge_into, vtx_key);
    }

    self
      .polygons
      .retain(|poly| !removed_face_keys.contains(&poly.key));
  }

  fn traverse_mut(self_key: NodeKey, nodes: &mut NodeMap, cb: &mut dyn FnMut(NodeKey, &mut Node)) {
    cb(self_key, &mut nodes[self_key]);
    if let Some(front_key) = nodes[self_key].front {
      Node::traverse_mut(front_key, nodes, cb);
    }
    if let Some(back_key) = nodes[self_key].back {
      Node::traverse_mut(back_key, nodes, cb);
    }
  }

  /// Consumes the BSP tree and returns a list of all polygons within it.
  fn into_polygons(self_key: NodeKey, nodes: &mut NodeMap) -> Vec<Polygon> {
    let mut polygons = Vec::new();
    Self::traverse_mut(self_key, nodes, &mut |_key, node| {
      polygons.extend(node.polygons.drain(..));
    });
    polygons
  }

  fn drain_polygons(self_key: NodeKey, nodes: &mut NodeMap, cb: &mut dyn FnMut(Polygon)) {
    Self::traverse_mut(self_key, nodes, &mut |_key, node| {
      for poly in node.polygons.drain(..) {
        cb(poly);
      }
    });
  }

  pub fn build(
    polygons: Vec<Polygon>,
    mesh: &mut LinkedMesh<FaceData>,
    nodes: &mut NodeMap,
  ) -> NodeKey {
    let dummy_node_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons,
    });
    Self::build_from_temp_node(dummy_node_key, mesh, nodes)
  }

  /// Build a BSP tree out of `polygons`. Each set of polygons is partitioned
  /// using the first polygon (no heuristic is used to pick a good split).
  pub fn build_from_temp_node(
    dummy_node_key: NodeKey,
    mesh: &mut LinkedMesh<FaceData>,
    nodes: &mut NodeMap,
  ) -> NodeKey {
    if nodes[dummy_node_key].polygons.is_empty() {
      panic!("No polygons in temp node");
    }
    let plane = nodes[dummy_node_key].polygons[0].plane(mesh).clone();

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
      );
    }

    let front = if nodes[temp_front_key].polygons.is_empty() {
      nodes.remove(temp_front_key);
      None
    } else {
      Some(Self::build_from_temp_node(temp_front_key, mesh, nodes))
    };
    let back = if nodes[temp_back_key].polygons.is_empty() {
      nodes.remove(temp_back_key);
      None
    } else {
      Some(Self::build_from_temp_node(temp_back_key, mesh, nodes))
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
    mesh: &mut LinkedMesh<FaceData>,
    nodes: &mut NodeMap,
  ) {
    // Add a dummy node to own the polygons so that we can handle pending polygons
    // getting split
    let dummy_node_key = nodes.insert(Node {
      plane: None,
      front: None,
      back: None,
      polygons: Vec::new(),
    });
    for poly in &mut polygons {
      poly.set_node_key(dummy_node_key, mesh);
    }
    nodes[dummy_node_key].polygons = polygons;

    Self::add_polygons_from_temp_node(self_key, dummy_node_key, mesh, nodes);
  }

  pub fn add_polygons_from_temp_node(
    self_key: NodeKey,
    dummy_node_key: NodeKey,
    mesh: &mut LinkedMesh<FaceData>,
    nodes: &mut NodeMap,
  ) {
    assert!(self_key != dummy_node_key);

    if nodes[dummy_node_key].polygons.is_empty() {
      return;
    }

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
        Some(front_key) => {
          Node::add_polygons_from_temp_node(front_key, temp_front_key, mesh, nodes)
        }
        None => {
          let new_front = Self::build_from_temp_node(temp_front_key, mesh, nodes);
          nodes[self_key].front = Some(new_front);
        }
      }
    }
    if nodes[temp_back_key].polygons.is_empty() {
      nodes.remove(temp_back_key);
    } else {
      match back_key {
        Some(back_key) => Node::add_polygons_from_temp_node(back_key, temp_back_key, mesh, nodes),
        None => {
          let new_back = Self::build_from_temp_node(temp_back_key, mesh, nodes);
          nodes[self_key].back = Some(new_back);
        }
      }
    }
  }
}

pub struct CSG {
  pub polygons: Vec<Polygon>,
  pub mesh: LinkedMesh<FaceData>,
}

impl CSG {
  pub fn new(polygons: Vec<Polygon>, mesh: LinkedMesh<FaceData>) -> Self {
    Self { polygons, mesh }
  }

  fn merge_other(mesh: &mut LinkedMesh<FaceData>, other: LinkedMesh<FaceData>) -> Vec<Polygon> {
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
      .into_iter()
      .map(|(_key, face)| {
        let vertices = [
          our_vtx_key_by_other_vtx_key[&face.vertices[0]],
          our_vtx_key_by_other_vtx_key[&face.vertices[1]],
          our_vtx_key_by_other_vtx_key[&face.vertices[2]],
        ];
        let face_key = mesh.add_face(vertices, face.data);
        Polygon::new(face_key)
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

  fn init(self, other: LinkedMesh<FaceData>) -> (LinkedMesh<FaceData>, NodeMap, NodeKey, NodeKey) {
    let mut nodes = Self::init_nodes();
    let mut mesh = self.mesh;
    let a_key = Node::build(self.polygons, &mut mesh, &mut nodes);

    let csg_polygons = Self::merge_other(&mut mesh, other);
    let b_key = Node::build(csg_polygons, &mut mesh, &mut nodes);

    (mesh, nodes, a_key, b_key)
  }

  fn extract_mesh(
    mut mesh: LinkedMesh<FaceData>,
    mut nodes: NodeMap,
    a_key: NodeKey,
  ) -> LinkedMesh<()> {
    Node::traverse_mut(a_key, &mut nodes, &mut |node_key, node| {
      node.collapse_interior_vertices(&mut mesh);
      node.remesh(node_key, &mut mesh)
    });

    let mut new_mesh: LinkedMesh<()> = LinkedMesh::default();
    let mut new_vtx_key_by_old_vtx_key = FxHashMap::default();
    Node::drain_polygons(a_key, &mut nodes, &mut |poly| {
      let mut face_vertices = [VertexKey::null(); 3];
      for (i, vtx_key) in mesh.faces[poly.key].vertices.into_iter().enumerate() {
        let vtx_key = *new_vtx_key_by_old_vtx_key
          .entry(vtx_key)
          .or_insert_with(|| {
            let position = mesh.vertices[vtx_key].position;
            new_mesh.vertices.insert(linked_mesh::Vertex {
              position,
              shading_normal: None,
              displacement_normal: None,
              edges: Vec::new(),
            })
          });
        face_vertices[i] = vtx_key;
      }
      new_mesh.add_face(face_vertices, ());
    });

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
  pub fn union(self, other: LinkedMesh<FaceData>) -> LinkedMesh {
    let (mut mesh, mut nodes, a_key, b_key) = self.init(other);

    Node::clip_to(a_key, b_key, &mut mesh, &mut nodes);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(a_key, b_polygons, &mut mesh, &mut nodes);

    Self::extract_mesh(mesh, nodes, a_key)
  }

  /// Removes all parts of `other` that are inside of `self` and returns the result.
  pub fn clip_to_self(self, other: LinkedMesh<FaceData>) -> LinkedMesh {
    let (mut mesh, mut nodes, a_key, b_key) = self.init(other);

    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(a_key, b_polygons, &mut mesh, &mut nodes);

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
  pub fn subtract(self, other: LinkedMesh<FaceData>) -> LinkedMesh {
    let (mut mesh, mut nodes, a_key, b_key) = self.init(other);

    Node::invert(a_key, &mut nodes, &mut mesh);
    Node::clip_to(a_key, b_key, &mut mesh, &mut nodes);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(a_key, b_polygons, &mut mesh, &mut nodes);
    Node::invert(a_key, &mut nodes, &mut mesh);

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
  pub fn intersect(self, csg: LinkedMesh<FaceData>) -> LinkedMesh<()> {
    let (mut mesh, mut nodes, a_key, b_key) = self.init(csg);

    Node::invert(a_key, &mut nodes, &mut mesh);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);
    Node::clip_to(a_key, b_key, &mut mesh, &mut nodes);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(a_key, b_polygons, &mut mesh, &mut nodes);
    Node::invert(a_key, &mut nodes, &mut mesh);

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

    let vtx_count = 4 * 6;
    let face_count = 6;
    let mut mesh = LinkedMesh::new(vtx_count, face_count, None);
    for face_vertices in &polygons {
      let mut polygon_vertices: [Vertex; 4] = uninit();
      for (vtx_ix, vtx) in face_vertices.into_iter().enumerate() {
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
        polygon_vertices[vtx_ix] = Vertex { key: vtx_key };
      }

      let plane = Plane::from_points(
        polygon_vertices[0].pos(&mesh),
        polygon_vertices[1].pos(&mesh),
        polygon_vertices[2].pos(&mesh),
      );
      for _ in triangulate_polygon(polygon_vertices.into(), &mut mesh, plane, NodeKey::null()) {
        // pass
      }
    }

    mesh.merge_vertices_by_distance(1e-5);
    let mut faces = Vec::with_capacity(mesh.faces.len());
    for (face_key, _face) in mesh.faces.iter() {
      faces.push(Polygon::new(face_key));
    }
    for poly in &faces {
      poly.compute_plane(&mut mesh);
    }
    Self::new(faces, mesh)
  }
}

impl From<LinkedMesh<FaceData>> for CSG {
  fn from(mut mesh: LinkedMesh<FaceData>) -> Self {
    let polygons: Vec<Polygon> = mesh
      .faces
      .iter()
      .map(|(face_key, _face)| Polygon::new(face_key))
      .collect();
    for poly in &polygons {
      poly.compute_plane(&mut mesh);
    }
    Self { polygons, mesh }
  }
}
