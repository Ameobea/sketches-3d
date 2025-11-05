//! A Rust port of the CSG.js library by Evan Wallace
//! (https://github.com/evanw/csg.js/).

use std::ops::BitOr;

use arrayvec::ArrayVec;
use common::uninit;
use fxhash::{FxBuildHasher, FxHashMap, FxHashSet};
use lyon_tessellation::{
  math::point, path::Path, FillGeometryBuilder, FillOptions, FillTessellator, GeometryBuilder,
};
use nalgebra::Point3;
use parry3d::{
  math::Isometry,
  shape::{TriMesh, TriMeshFlags},
  transformation::{
    intersect_meshes_with_tolerances, MeshIntersectionError, MeshIntersectionTolerances,
  },
};
use slotmap::Key;
use smallvec::SmallVec;

use crate::{
  linked_mesh::{self, DisplacementNormalMethod, Edge, EdgeSplitPos, FaceKey, Vec3, VertexKey},
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

#[derive(Clone, Debug)]
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

// I'm pretty sure this only works for convex polygons
fn triangulate_polygon<'a>(
  vertices: ArrayVec<VertexKey, 4>,
  mesh: &'a mut LinkedMesh<FaceData>,
  plane: &'a Plane,
  node_key: NodeKey,
) -> impl Iterator<Item = Polygon> + 'a {
  (2..vertices.len()).map(move |i| {
    let face_key = mesh.add_face::<false>(
      [vertices[0], vertices[i - 1], vertices[i]],
      FaceData {
        plane: plane.clone(),
        node_key,
      },
    );
    Polygon { key: face_key }
  })
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PolygonClass {
  Coplanar = 0,
  Front = 1,
  Back = 2,
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

fn vtx_pos<T>(vtx_key: VertexKey, mesh: &LinkedMesh<T>) -> Vec3 {
  if cfg!(feature = "unsafe_indexing") {
    unsafe { mesh.vertices.get_unchecked(vtx_key).position }
  } else {
    mesh.vertices[vtx_key].position
  }
}

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
      // if mesh.faces[new_face_keys[face_ix]].is_degenerate(&mesh.vertices) {
      //   log::warn!("Dropping degenerate face with key={new_face_key:?}");
      //   mesh.remove_face(new_face_key);
      //   return None;
      // }

      let poly = Polygon { key: new_face_key };
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
  pub fn to_2d(&self, point: Vec3, u: &Vec3, v: &Vec3) -> [f32; 2] {
    let point_on_plane = self.point_on_plane();
    let relative_point = point - point_on_plane;
    let x = relative_point.dot(u);
    let y = relative_point.dot(v);
    [x, y]
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
    for (vtx_ix, vtx) in mesh.faces[polygon.key].vertices.iter().copied().enumerate() {
      let t = self.normal.dot(&vtx_pos(vtx, mesh)) - self.w;
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
        let mut f = ArrayVec::<VertexKey, 4>::new();
        let mut b = ArrayVec::<VertexKey, 4>::new();
        let mut split_vertices = ArrayVec::<_, 2>::new();

        let verts = mesh.faces[polygon.key].vertices;
        let old_poly_user_data = mesh.remove_face(polygon.key);
        let split_faces = get_split_face_scratch();
        assert!(split_faces.is_empty());

        for i in 0..3 {
          let j = (i + 1) % 3;
          let ti = types[i];
          let tj = types[j];
          let vi_key = verts[i];
          let vj_key = verts[j];

          if ti != PolygonClass::Back {
            f.push(vi_key);
          }
          if ti != PolygonClass::Front {
            b.push(vi_key);
          }
          if (ti | tj) == PolygonClass::Spanning {
            let vi = &mesh.vertices[vi_key];
            let vj = &mesh.vertices[vj_key];
            let t = (self.w - self.normal.dot(&vi.position))
              / self.normal.dot(&(vj.position - vi.position));

            let middle_vtx_key = if let Some(edge_key) = mesh.get_edge_key([vi_key, vj_key]) {
              mesh.split_edge_cb(
                edge_key,
                EdgeSplitPos {
                  pos: t,
                  start_vtx_key: vi_key,
                },
                DisplacementNormalMethod::Interpolate,
                |_mesh, old_face_key, old_face_data, new_face_keys| {
                  split_faces.push(((old_face_key, old_face_data), new_face_keys))
                },
              )
            } else {
              // The face we're splitting is the only one that uses this edge, we can just
              // add the new vertex to the mesh
              let position = vi.position.lerp(&vj.position, t);
              let shading_normal = match (vi.shading_normal, vj.shading_normal) {
                (Some(n0), Some(n1)) => Some(n0.lerp(&n1, t).normalize()),
                _ => None,
              };
              let displacement_normal = match (vi.displacement_normal, vj.displacement_normal) {
                (Some(n0), Some(n1)) => Some(n0.lerp(&n1, t).normalize()),
                _ => None,
              };

              mesh.vertices.insert(linked_mesh::Vertex {
                position,
                shading_normal,
                displacement_normal,
                edges: SmallVec::new(),
                _padding: Default::default(),
              })
            };

            f.push(middle_vtx_key);
            b.push(middle_vtx_key);
            split_vertices.push(middle_vtx_key);
          }
        }

        if f.len() >= 3 {
          nodes[front_key].polygons.extend(triangulate_polygon(
            f,
            mesh,
            &old_poly_user_data.plane,
            front_key,
          ));
        }
        if b.len() >= 3 {
          nodes[back_key].polygons.extend(triangulate_polygon(
            b,
            mesh,
            &old_poly_user_data.plane,
            back_key,
          ));
        }

        handle_split_faces(split_faces, mesh, nodes);

        if let Some(_plane_node_key) = plane_node_key {
          // weld_polygons(&split_vertices, plane_node_key, mesh, nodes);
        }
      }
    }
  }
}

#[derive(Debug)]
pub struct Polygon {
  pub key: FaceKey,
}

impl Polygon {
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
    let user_data = self.user_data_mut(mesh);
    user_data.plane.flip();
    let verts = &mut mesh.faces[self.key].vertices;
    verts.swap(0, 2);
  }

  fn compute_plane(&self, mesh: &mut LinkedMesh<FaceData>) {
    let [v0_pos, v1_pos, v2_pos] = self.vtx_coords(mesh);
    *self.plane_mut(mesh) = Plane::from_points(v0_pos, v1_pos, v2_pos);
  }

  fn set_node_key(&self, node_key: NodeKey, mesh: &mut LinkedMesh<FaceData>) {
    self.user_data_mut(mesh).node_key = node_key;
  }

  #[cfg(feature = "broken-csg-welding")]
  fn vtx2(&self, ix0: usize, ix1: usize, mesh: &LinkedMesh<FaceData>) -> [VertexKey; 2] {
    let face = if cfg!(feature = "unsafe_indexing") {
      unsafe { mesh.faces.get_unchecked(self.key) }
    } else {
      &mesh.faces[self.key]
    };
    [face.vertices[ix0], face.vertices[ix1]]
  }

  fn vtx_coords(&self, mesh: &LinkedMesh<FaceData>) -> [Vec3; 3] {
    if cfg!(feature = "unsafe_indexing") {
      unsafe {
        let face = mesh.faces.get_unchecked(self.key);
        [
          mesh.vertices.get_unchecked(face.vertices[0]).position,
          mesh.vertices.get_unchecked(face.vertices[1]).position,
          mesh.vertices.get_unchecked(face.vertices[2]).position,
        ]
      }
    } else {
      let face = &mesh.faces[self.key];
      [
        mesh.vertices[face.vertices[0]].position,
        mesh.vertices[face.vertices[1]].position,
        mesh.vertices[face.vertices[2]].position,
      ]
    }
  }
}

#[cfg(feature = "broken-csg-welding")]
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

#[cfg(feature = "broken-csg-welding")]
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

#[cfg(feature = "broken-csg-welding")]
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

#[cfg(feature = "broken-csg-welding")]
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
#[cfg(feature = "broken-csg-welding")]
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

#[cfg(feature = "broken-csg-welding")]
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

#[cfg(feature = "broken-csg-welding")]
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

#[cfg(feature = "broken-csg-welding")]
fn weld_polygon_at_interior<'a>(
  vtx: VertexKey,
  poly: &Polygon,
  mesh: &'a mut LinkedMesh<FaceData>,
) -> impl Iterator<Item = Polygon> + 'a {
  let verts = &mesh.faces[poly.key].vertices;
  let verts = [
    [verts[0], verts[1], vtx],
    [verts[2], vtx, verts[1]],
    [verts[2], verts[0], vtx],
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
    Polygon { key: face_key }
  })
}

/// `edge_pos` is the interpolation factor between the two vertices that the point is on
#[cfg(feature = "broken-csg-welding")]
fn weld_polygon_on_edge<'a>(
  out_temp_node_key: NodeKey,
  poly: Polygon,
  edge_ix: u8,
  mesh: &'a mut LinkedMesh<FaceData>,
  nodes: &'a mut NodeMap,
  edge_pos: f32,
) -> impl Iterator<Item = Polygon> + 'a {
  let [v0, v1] = match edge_ix {
    0 => poly.vtx2(0, 1, mesh),
    1 => poly.vtx2(1, 2, mesh),
    2 => poly.vtx2(2, 0, mesh),
    _ => unreachable!(),
  };

  let split_faces = get_split_face_scratch();
  assert!(split_faces.is_empty());

  let edge = mesh
    .get_edge_key([v0, v1])
    .unwrap_or_else(|| panic!("Couldn't find edge key for vertices {v0:?} and {v1:?}",));

  // Need to move the polygon into the temporary node so that we can split it
  let poly_key = poly.key;
  poly.set_node_key(out_temp_node_key, mesh);
  nodes[out_temp_node_key].polygons.push(poly);

  mesh.split_edge_cb(
    edge,
    EdgeSplitPos {
      pos: edge_pos,
      start_vtx_key: v0,
    },
    DisplacementNormalMethod::Interpolate,
    |_mesh, old_face_key, old_face_data, new_face_keys| {
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
#[cfg(feature = "broken-csg-welding")]
fn maybe_weld_polygon(
  vtx: VertexKey,
  out_tmp_key: NodeKey,
  poly: Polygon,
  mesh: &mut LinkedMesh<FaceData>,
  nodes: &mut NodeMap,
) -> ArrayVec<Polygon, 3> {
  let vert_coords = poly.vtx_coords(mesh);

  // the epsilon value below seems to matter quite a bit.  The triangle intersection test seems
  // quite prone to floating point precision issues.
  let ixn = triangle_contains_point(&vert_coords, vtx_pos(vtx, mesh), 1e-4);
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

#[cfg(feature = "broken-csg-welding")]
fn weld_polygons(
  split_vertices: &[VertexKey],
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
    preserve_from_node: bool,
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

    if !preserve_from_node {
      assert!(nodes.remove(from_key).unwrap().polygons.is_empty());
    }

    let mut front;
    let mut back = Vec::new();

    if let Some(front_key) = front_key {
      front = Node::clip_polygons(front_key, temp_front_key, mesh, nodes, true);

      // we have to put them back into a temp node because clipping the back polys might cause some
      // of them to be split
      assert!(nodes[temp_front_key].polygons.is_empty());
      for poly in &front {
        poly.set_node_key(temp_front_key, mesh);
      }
      nodes[temp_front_key].polygons = front;
    }
    if let Some(back_key) = back_key {
      back = Node::clip_polygons(back_key, temp_back_key, mesh, nodes, false);
    } else {
      for poly in nodes.remove(temp_back_key).unwrap().polygons {
        mesh.remove_face(poly.key);
      }
    }

    front = nodes.remove(temp_front_key).unwrap().polygons;

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
    let new_this_polygons = Node::clip_polygons(bsp_key, self_key, mesh, nodes, true);

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

  fn compute_perimeter(
    &self,
    mesh: &LinkedMesh<FaceData>,
  ) -> Option<(Vec<VertexKey>, Vec<VertexKey>)> {
    // don't bother re-meshing already trivial polygons
    if self.polygons.len() < 3 {
      return None;
    }

    let all_vtx_keys: FxHashSet<_> = self
      .polygons
      .iter()
      .flat_map(|poly| mesh.faces[poly.key].vertices)
      .collect();

    let all_face_keys = self
      .polygons
      .iter()
      .map(|poly| poly.key)
      .collect::<FxHashSet<_>>();

    fn is_boundary_edge(edge: &Edge, all_face_keys: &FxHashSet<FaceKey>) -> bool {
      edge
        .faces
        .iter()
        .filter(|&face| all_face_keys.contains(face))
        .count()
        == 1
    }

    fn is_interior_vtx(
      vtx_key: VertexKey,
      mesh: &LinkedMesh<FaceData>,
      all_face_keys: &FxHashSet<FaceKey>,
    ) -> bool {
      let vtx = &mesh.vertices[vtx_key];
      vtx.edges.iter().all(|&edge_key| {
        let edge = &mesh.edges[edge_key];
        !is_boundary_edge(edge, all_face_keys)
      })
    }

    let interior_vtx_keys: Vec<VertexKey> = all_vtx_keys
      .iter()
      .filter(|&vtx_key| is_interior_vtx(*vtx_key, mesh, &all_face_keys))
      .copied()
      .collect();
    let boundary_vtx_count = all_vtx_keys.len() - interior_vtx_keys.len();

    let Some((start_face_key, start_edge_key)) = all_face_keys.iter().find_map(|&face_key| {
      for edge in mesh.faces[face_key].edges {
        let mut contained_face_count = 0usize;
        for face in &mesh.edges[edge].faces {
          if all_face_keys.contains(face) {
            contained_face_count += 1;
            if contained_face_count > 1 {
              return None;
            }
          }
        }

        assert!(contained_face_count > 0);
        if contained_face_count == 1 {
          return Some((face_key, edge));
        }
      }

      None
    }) else {
      log::warn!("No boundary edges found?");
      return None;
    };

    // start off walking in the correct direction
    let (start_vtx_key, next_vtx_key) = {
      let edge = &mesh.edges[start_edge_key];
      let face = &mesh.faces[start_face_key];

      let vtx0_ix = face
        .vertices
        .iter()
        .position(|&vtx_key| vtx_key == edge.vertices[0])
        .unwrap();
      match vtx0_ix {
        0 => {
          if face.vertices[1] == edge.vertices[0] {
            (edge.vertices[0], edge.vertices[1])
          } else {
            (edge.vertices[1], edge.vertices[0])
          }
        }
        1 => {
          if face.vertices[2] == edge.vertices[0] {
            (edge.vertices[0], edge.vertices[1])
          } else {
            (edge.vertices[1], edge.vertices[0])
          }
        }
        2 => {
          if face.vertices[0] == edge.vertices[0] {
            (edge.vertices[0], edge.vertices[1])
          } else {
            (edge.vertices[1], edge.vertices[0])
          }
        }
        _ => unreachable!(),
      }
    };

    // walk the perimeter of the polygon to build up an ordered list of vertices
    let mut visited_vtx_keys = FxHashSet::default();
    visited_vtx_keys.insert(start_vtx_key);
    visited_vtx_keys.insert(next_vtx_key);
    let mut perimeter: Vec<VertexKey> = Vec::new();
    perimeter.push(start_vtx_key);
    perimeter.push(next_vtx_key);

    let mut cur_vtx_key = next_vtx_key;
    loop {
      let vtx = &mesh.vertices[cur_vtx_key];
      let next_vtx_key = vtx.edges.iter().find_map(|&edge_key| {
        let other_vtx = mesh.edges[edge_key].other_vtx(cur_vtx_key);
        if !all_vtx_keys.contains(&other_vtx) || visited_vtx_keys.contains(&other_vtx) {
          return None;
        }

        // only consider boundary edges.  A boundary edge is one that has only one face in
        // `all_face_keys`.
        let mut contained_face_count = 0usize;
        for face in &mesh.edges[edge_key].faces {
          if all_face_keys.contains(face) {
            contained_face_count += 1;
            if contained_face_count > 1 {
              return None;
            }
          }
        }

        Some(other_vtx)
      });

      let Some(next_vtx_key) = next_vtx_key else {
        break;
      };
      cur_vtx_key = next_vtx_key;
      perimeter.push(cur_vtx_key);
      visited_vtx_keys.insert(cur_vtx_key);
    }

    // `cur_vtx_key` should connect back to `start_vtx_key`
    let perimeter_is_closed = mesh.vertices[cur_vtx_key].edges.iter().any(|&edge_key| {
      let other_vtx = mesh.edges[edge_key].other_vtx(cur_vtx_key);
      other_vtx == start_vtx_key
    });
    // assert!(perimeter_is_closed, "Perimeter is not closed");
    if !perimeter_is_closed {
      log::warn!("Perimeter is not closed");
      return None;
    }

    if perimeter.len() != boundary_vtx_count {
      log::warn!(
        "Perimeter has {} vertices, but there are {boundary_vtx_count} boundary vertices.  \
         Probably has holes or other complicated geometry",
        perimeter.len()
      );
      return None;
    }

    Some((perimeter, interior_vtx_keys))
  }

  fn remesh_lyon(&mut self, self_key: NodeKey, mesh: &mut LinkedMesh<FaceData>) {
    let Some((perimeter, interior_vtx_keys)) = self.compute_perimeter(mesh) else {
      return;
    };

    // lyon tessellates 2D polygons, so we use the plane to project the vertices into 2D space
    //
    // they can be projected back using these vectors after we re-add the new path
    let plane = self.polygons[0].plane(mesh).clone();
    let (u, v) = plane.compute_basis();

    // remove all faces and interior vertices
    // let before_face_count = self.polygons.len();
    for poly in self.polygons.drain(..) {
      mesh.remove_face(poly.key);
    }
    for vtx_key in interior_vtx_keys {
      assert!(
        mesh.vertices.remove(vtx_key).unwrap().edges.is_empty(),
        "Interior vertex has edges"
      );
    }

    let to_2d = |vtx_key: VertexKey| {
      let pos = mesh.vertices[vtx_key].position;
      let [x, y] = plane.to_2d(pos, &u, &v);
      point(x, y)
    };

    let mut path_builder = Path::builder();
    path_builder.begin(to_2d(perimeter[0]));
    for &vtx_key in &perimeter[1..] {
      path_builder.line_to(to_2d(vtx_key));
    }
    path_builder.end(true);
    let path: Path = path_builder.build();

    struct CustomBuilder<'a> {
      perimeter: Vec<VertexKey>,
      vertices: Vec<VertexKey>,
      mesh: &'a mut LinkedMesh<FaceData>,
      polys: Vec<Polygon>,
      plane: Plane,
      self_key: NodeKey,
    }

    impl<'a> GeometryBuilder for CustomBuilder<'a> {
      fn add_triangle(
        &mut self,
        a: lyon_tessellation::VertexId,
        b: lyon_tessellation::VertexId,
        c: lyon_tessellation::VertexId,
      ) {
        // idk why these are flipped
        let a = self.vertices[a.0 as usize];
        let b = self.vertices[b.0 as usize];
        let c = self.vertices[c.0 as usize];
        let face_key = self.mesh.add_face::<false>(
          [c, b, a],
          FaceData {
            plane: self.plane.clone(),
            node_key: self.self_key,
          },
        );
        self.polys.push(Polygon { key: face_key });
      }
    }

    impl<'a> FillGeometryBuilder for CustomBuilder<'a> {
      fn add_fill_vertex(
        &mut self,
        vertex: lyon_tessellation::FillVertex,
      ) -> Result<lyon_tessellation::VertexId, lyon_tessellation::GeometryBuilderError> {
        let vtx_ix = vertex
          .as_endpoint_id()
          .expect("no endpoint found")
          .to_usize();
        let vtx_key = self.perimeter[vtx_ix];

        self.vertices.push(vtx_key);
        Ok(lyon_tessellation::VertexId(
          (self.vertices.len() - 1) as u32,
        ))
      }
    }

    let mut vertex_builder = CustomBuilder {
      perimeter,
      vertices: Vec::new(),
      mesh,
      polys: Vec::new(),
      plane,
      self_key,
    };

    let mut tessellator = FillTessellator::new();

    tessellator
      .tessellate_with_ids(
        path.id_iter(),
        &path,
        Some(&path),
        &FillOptions::default()
          .with_tolerance(0.00000000001)
          .with_intersections(false),
        &mut vertex_builder,
      )
      .expect("tessellation failed");

    let new_polys = vertex_builder.polys;
    // let after_face_count = new_polys.len();
    // log::info!("lyon: {before_face_count} -> {after_face_count} faces for
    // node_key={self_key:?}");

    self.polygons = new_polys;
  }

  fn traverse_mut(self_key: NodeKey, nodes: &mut NodeMap, cb: &mut impl FnMut(NodeKey, &mut Node)) {
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
      polygons.append(&mut node.polygons);
    });
    polygons
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
    for poly in &nodes[dummy_node_key].polygons {
      poly.set_node_key(dummy_node_key, mesh);
    }
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
    // TODO: figure out a good heuristic for picking the split plane
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
      plane: Some(plane.clone()),
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
        shading_normal: vtx.shading_normal,
        displacement_normal: vtx.displacement_normal,
        edges: SmallVec::new(),
        _padding: Default::default(),
      });
      our_vtx_key_by_other_vtx_key.insert(vtx_key, new_key);
    }
    other
      .faces
      .into_iter()
      .map(|(_key, face)| {
        let vertices = [
          our_vtx_key_by_other_vtx_key[&face.vertices[0]],
          our_vtx_key_by_other_vtx_key[&face.vertices[1]],
          our_vtx_key_by_other_vtx_key[&face.vertices[2]],
        ];
        let face_key = mesh.add_face::<false>(vertices, face.data);
        Polygon { key: face_key }
      })
      .collect::<Vec<_>>()
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

  fn build_trimeshes(self: CSG, other: LinkedMesh<FaceData>) -> (TriMesh, TriMesh) {
    fn populate_trimesh(
      verts: &mut Vec<Point3<f32>>,
      indices: &mut Vec<[u32; 3]>,
      mesh: &LinkedMesh<FaceData>,
    ) {
      let mut vtx_ix_by_key =
        FxHashMap::with_capacity_and_hasher(mesh.vertices.len(), FxBuildHasher::default());

      for face in mesh.faces.values() {
        let ixs = std::array::from_fn(|i| {
          let vtx_key = face.vertices[i];
          *vtx_ix_by_key.entry(vtx_key).or_insert_with(|| {
            let ix = verts.len();
            verts.push(mesh.vertices[vtx_key].position.into());
            ix as u32
          })
        });
        indices.push(ixs);
      }
    }

    let flags = TriMeshFlags::default() | TriMeshFlags::ORIENTED | TriMeshFlags::HALF_EDGE_TOPOLOGY;

    let mut a_verts = Vec::with_capacity(self.mesh.vertices.len());
    let mut a_indices = Vec::with_capacity(self.mesh.faces.len());
    populate_trimesh(&mut a_verts, &mut a_indices, &self.mesh);
    let a_trimesh =
      TriMesh::with_flags(a_verts, a_indices, flags).expect("Failed to build trimesh for A");

    let mut b_verts = Vec::with_capacity(other.vertices.len());
    let mut b_indices = Vec::with_capacity(other.faces.len());
    populate_trimesh(&mut b_verts, &mut b_indices, &other);
    let b_trimesh =
      TriMesh::with_flags(b_verts, b_indices, flags).expect("Failed to build trimesh for B");

    (a_trimesh, b_trimesh)
  }

  fn remesh(mesh: &mut LinkedMesh<FaceData>, nodes: &mut NodeMap, a_key: NodeKey) {
    Node::traverse_mut(a_key, nodes, &mut |node_key, node| {
      node.remesh_lyon(node_key, mesh);
      // node.remesh_earcut(node_key, &mut mesh);
    });
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
  pub fn union(self, other: LinkedMesh<FaceData>) -> LinkedMesh<FaceData> {
    let (mut mesh, mut nodes, a_key, b_key) = self.init(other);

    Node::clip_to(a_key, b_key, &mut mesh, &mut nodes);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(a_key, b_polygons, &mut mesh, &mut nodes);

    CSG::remesh(&mut mesh, &mut nodes, a_key);
    mesh
  }

  /// Removes all parts of `other` that are inside of `self` and returns the result.
  pub fn clip_to_self(self, other: LinkedMesh<FaceData>) -> LinkedMesh<FaceData> {
    let (mut mesh, mut nodes, a_key, b_key) = self.init(other);

    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(a_key, b_polygons, &mut mesh, &mut nodes);

    CSG::remesh(&mut mesh, &mut nodes, a_key);
    mesh
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
  pub fn subtract(self, other: LinkedMesh<FaceData>) -> LinkedMesh<FaceData> {
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

    CSG::remesh(&mut mesh, &mut nodes, a_key);
    mesh
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
  pub fn intersect(self, csg: LinkedMesh<FaceData>) -> LinkedMesh<FaceData> {
    let (mut mesh, mut nodes, a_key, b_key) = self.init(csg);

    Node::invert(a_key, &mut nodes, &mut mesh);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);
    Node::invert(b_key, &mut nodes, &mut mesh);
    Node::clip_to(a_key, b_key, &mut mesh, &mut nodes);
    Node::clip_to(b_key, a_key, &mut mesh, &mut nodes);

    let b_polygons = Node::into_polygons(b_key, &mut nodes);
    Node::add_polygons(a_key, b_polygons, &mut mesh, &mut nodes);
    Node::invert(a_key, &mut nodes, &mut mesh);

    CSG::remesh(&mut mesh, &mut nodes, a_key);
    mesh
  }

  pub fn intersect_experimental(
    // TODO: Switch to accepting indices and vertices directly
    self,
    csg: LinkedMesh<FaceData>,
  ) -> Result<LinkedMesh<()>, MeshIntersectionError> {
    let (a_trimesh, b_trimesh) = Self::build_trimeshes(self, csg);

    let mut tolerances = MeshIntersectionTolerances::default();
    // tolerances.angle_epsilon = 0.00000000001;
    // tolerances.collinearity_epsilon = std::f32::EPSILON * 0.000001;
    tolerances.global_insertion_epsilon = 0.000000000001;
    // tolerances.local_insertion_epsilon_scale = 1.;
    let opt = intersect_meshes_with_tolerances(
      &Isometry::translation(-1.423 + 0.000001, 5.4423423, 2.2334283432),
      &a_trimesh,
      false,
      &Isometry::translation(0., 0., 0.),
      &b_trimesh,
      true,
      tolerances,
    )?;
    let Some(out_mesh) = opt else {
      log::warn!("`None` returned from `intersect_meshes`");
      return Ok(LinkedMesh::new(0, 0, None));
    };

    let mut mesh: LinkedMesh<()> = out_mesh.into();
    mesh.merge_vertices_by_distance(0.00001);

    log::info!(
      "PRE CLEANUP mesh topology check: 1-manifold={:?}; 2-manifold={:?}",
      mesh.check_is_manifold::<false>(),
      mesh.check_is_manifold::<true>()
    );

    fn cleanup_mesh(mesh: &mut LinkedMesh<()>) -> usize {
      const MIN_ANGLE_RADS: f32 = 0.01;

      let mut removed_face_count = 0usize;

      let all_face_keys = mesh.faces.keys().collect::<Vec<_>>();
      'face: for face_key in all_face_keys {
        let verts_to_merge = 'verts: {
          let Some(face) = mesh.faces.get(face_key) else {
            // face was possibly removed by a previous operation
            continue 'face;
          };

          if face.is_degenerate(&mesh.vertices) {
            log::info!("Removing degenerate face: {face_key:?}");
            mesh.remove_face(face_key);
            removed_face_count += 1;
            continue 'face;
          }

          // sometimes weird flag triangles that are stuck to the mesh get generated
          let border_edge_count = face
            .edges
            .iter()
            .filter(|&&edge_key| {
              let edge = &mesh.edges[edge_key];
              edge.faces.len() == 1
            })
            .count();
          if border_edge_count >= 2 {
            log::info!(
              "Removing flag face: {face_key:?}; area={}",
              face.area(&mesh.vertices)
            );
            mesh.remove_face(face_key);
            removed_face_count += 1;
            continue 'face;
          }

          let vtx_0_angle = face.compute_angle_at_vertex(0, &mesh.vertices);
          if vtx_0_angle < MIN_ANGLE_RADS {
            log::info!(
              "v0 angle: {vtx_0_angle}; vtxs: {:?}",
              face.vertex_positions(&mesh.vertices)
            );
            break 'verts (face.vertices[1], face.vertices[2]);
          }

          let vtx_1_angle = face.compute_angle_at_vertex(1, &mesh.vertices);
          if vtx_1_angle < MIN_ANGLE_RADS {
            log::info!(
              "v1 angle: {vtx_1_angle}; vtxs: {:?}",
              face.vertex_positions(&mesh.vertices)
            );
            break 'verts (face.vertices[0], face.vertices[2]);
          }

          let vtx_2_angle = face.compute_angle_at_vertex(2, &mesh.vertices);
          if vtx_2_angle < MIN_ANGLE_RADS {
            log::info!(
              "v2 angle: {vtx_2_angle}; vtxs: {:?}",
              face.vertex_positions(&mesh.vertices)
            );
            break 'verts (face.vertices[0], face.vertices[1]);
          }

          continue 'face;
        };

        // TODO: check for triangles with tiny area

        log::info!(
          "Merging vertices {:?} in face {:?}",
          verts_to_merge,
          face_key
        );
        // TODO: make sure this is valid (pretty sure it's not, it's tearing holes in the mesh)
        // remove all faces that include the edge we're collapsing
        let Some(edge_key_to_remove) = mesh.get_edge_key([verts_to_merge.0, verts_to_merge.1])
        else {
          log::error!(
            "No edge found for vertices {verts_to_merge:?} in face {face_key:?} yet they appear \
             in the same face?"
          );
          continue;
        };
        let face_keys_to_remove = mesh.edges[edge_key_to_remove].faces.clone();
        log::info!(
          "Face count for edge being collapsed: {}",
          face_keys_to_remove.len()
        );
        if face_keys_to_remove.len() > 2 {
          continue;
        }
        for face_key in face_keys_to_remove {
          mesh.remove_face(face_key);
          removed_face_count += 1;
        }
        mesh.merge_vertices(verts_to_merge.0, verts_to_merge.1);
      }

      removed_face_count
    }

    loop {
      let removed_face_count = cleanup_mesh(&mut mesh);
      if removed_face_count == 0 {
        break;
      }
      log::info!("Removed {removed_face_count} faces");
    }

    mesh.vertices.retain(|_key, vtx| !vtx.edges.is_empty());

    mesh.merge_vertices_by_distance(0.00001);

    log::info!(
      "mesh topology check: 1-manifold={:?}; 2-manifold={:?}",
      mesh.check_is_manifold::<false>(),
      mesh.check_is_manifold::<true>()
    );

    // // TODO TEMP REMOVE
    // if let Err(NonManifoldError::NonManifoldEdge { edge_key, .. }) =
    //   mesh.check_is_manifold::<false>()
    // {
    //   let face_keys = mesh.edges[edge_key]
    //     .faces
    //     .iter()
    //     .copied()
    //     .collect::<FxHashSet<_>>();

    //   log::info!("BAD FACES:");
    //   //
    //   for &face_key in &face_keys {
    //     log::info!(
    //       " {:?}; area: {:?}",
    //       mesh.faces[face_key].vertex_positions(&mesh.vertices),
    //       mesh.faces[face_key].area(&mesh.vertices),
    //     );
    //   }

    //   let all_face_keys = mesh.faces.keys().collect::<Vec<_>>();
    //   for face_key in all_face_keys {
    //     if face_keys.contains(&face_key) {
    //       continue;
    //     }

    //     mesh.remove_face(face_key);
    //   }
    // }

    Ok(mesh)
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
      let mut polygon_vertices: [VertexKey; 4] = uninit();
      for (vtx_ix, vtx) in face_vertices.into_iter().enumerate() {
        let pos = Vec3::new(
          center[0] + radius * vtx[0] as f32,
          center[1] + radius * vtx[1] as f32,
          center[2] + radius * vtx[2] as f32,
        );
        let vtx_key = mesh.vertices.insert(linked_mesh::Vertex {
          position: pos,
          ..Default::default()
        });
        polygon_vertices[vtx_ix] = vtx_key;
      }

      let plane = Plane::from_points(
        vtx_pos(polygon_vertices[0], &mesh),
        vtx_pos(polygon_vertices[1], &mesh),
        vtx_pos(polygon_vertices[2], &mesh),
      );
      for _ in triangulate_polygon(polygon_vertices.into(), &mut mesh, &plane, NodeKey::null()) {
        // pass
      }
    }

    mesh.merge_vertices_by_distance(1e-5);
    mesh.mark_edge_sharpness(0.8);
    mesh.separate_vertices_and_compute_normals();

    let mut faces = Vec::with_capacity(mesh.faces.len());
    for (face_key, _face) in mesh.faces.iter() {
      faces.push(Polygon { key: face_key });
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
      .map(|(face_key, _face)| Polygon { key: face_key })
      .collect();
    for poly in &polygons {
      poly.compute_plane(&mut mesh);
    }
    Self { polygons, mesh }
  }
}
