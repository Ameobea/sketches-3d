use std::{
  f32::consts::PI, fmt::Debug, hash::Hash, marker::PhantomData, mem::MaybeUninit, num::NonZeroU32,
};

use arrayvec::ArrayVec;
use bitvec::{bitarr, slice::BitSlice};
use fxhash::{FxBuildHasher, FxHashMap, FxHashSet};
use nalgebra::{Matrix4, Vector3};
use parry3d::{
  bounding_volume::Aabb,
  math::Point,
  shape::{TriMesh, TriMeshBuilderError},
};
use slotmap::{new_key_type, Key, SlotMap};
use smallvec::SmallVec;

use crate::{
  models::{
    stanford_bunny::{STANFORD_BUNNY_INDICES, STANFORD_BUNNY_VERTICES},
    utah_teapot::{UTAH_TEAPOT_INDICES, UTAH_TEAPOT_VERTICES},
  },
  slotmap_utils::{
    build_slotmap_from_iter, build_slotmap_from_iter_with_key, ekey_ix, slotmap_insert_dense,
    slotmap_insert_dense_with_key, vkey, vkey_ix, vkey_ix_mut, vkey_version, LocalKeyData,
    LocalSlotMap,
  },
  OwnedIndexedMesh, OwnedIndexedMeshBuilder, OwnedMesh, Triangle,
};

pub type Vec3 = Vector3<f32>;
pub type Mat4 = Matrix4<f32>;

new_key_type! {
  pub struct VertexKey;
  pub struct FaceKey;
  pub struct EdgeKey;
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct VtxPadding(MaybeUninit<[u8; 4]>);

impl Default for VtxPadding {
  fn default() -> Self {
    VtxPadding(MaybeUninit::uninit())
  }
}

#[derive(Clone, Debug, Default)]
// #[repr(align(128))]
pub struct Vertex {
  pub position: Vec3,
  /// Normal of the vertex used for shading/lighting.
  pub shading_normal: Option<Vec3>,
  /// Normal of the vertex used for displacement mapping.
  pub displacement_normal: Option<Vec3>,
  // 9 is the max size of the inline buffer we can set while keeping `Vertex` size under 128 bytes
  pub edges: SmallVec<[EdgeKey; 9]>,
  pub _padding: VtxPadding,
}

#[derive(Clone, Debug)]
pub struct Face<T> {
  /// Counter-clockwise winding
  pub vertices: [VertexKey; 3],
  /// Unordered
  pub edges: [EdgeKey; 3],
  pub data: T,
}

static mut GRAPHVIZ_PRINT_INNER: fn(&str) -> () = |s| {
  println!("{}", s);
};

fn graphviz_print(s: &str) {
  unsafe {
    GRAPHVIZ_PRINT_INNER(s);
  }
}

pub fn set_graphviz_print(f: fn(&str) -> ()) {
  unsafe {
    GRAPHVIZ_PRINT_INNER = f;
  }
}

static mut DEBUG_PRINT_INNER: fn(&str) -> () = |s| {
  println!("{s}");
};

#[allow(dead_code)]
fn debug_print(s: &str) {
  unsafe {
    DEBUG_PRINT_INNER(s);
  }
}

pub fn set_debug_print(f: fn(&str) -> ()) {
  unsafe {
    DEBUG_PRINT_INNER = f;
  }
}

// /// Same as Vec::retain but it doesn't preserve order of elements and can possibly work a bit
// faster /// if only removing a few elements.
// fn swap_retain<T>(vec: &mut Vec<T>, mut f: impl FnMut(&mut T) -> bool) {
//   let mut i = 0;
//   while i < vec.len() {
//     if !f(&mut vec[i]) {
//       vec.swap_remove(i);
//     } else {
//       i += 1;
//     }
//   }
// }

/// Same as SmallVec::retain but it doesn't preserve order of elements and can possibly work a bit
/// faster if only removing a few elements.
fn swap_retain_sv<T: smallvec::Array>(
  vec: &mut SmallVec<T>,
  mut f: impl FnMut(&mut <T as smallvec::Array>::Item) -> bool,
) {
  let mut i = 0;
  while i < vec.len() {
    if !f(&mut vec[i]) {
      vec.swap_remove(i);
    } else {
      i += 1;
    }
  }
}

impl<T> Face<T> {
  pub fn vertex_positions(&self, verts: &SlotMap<VertexKey, Vertex>) -> [Vec3; 3] {
    [
      verts[self.vertices[0]].position,
      verts[self.vertices[1]].position,
      verts[self.vertices[2]].position,
    ]
  }

  pub fn iter_vtx_positions<'a>(
    &'a self,
    verts: &'a SlotMap<VertexKey, Vertex>,
  ) -> impl Iterator<Item = &'a Vec3> + 'a {
    self
      .vertices
      .iter()
      .map(move |&vtx_key| &verts[vtx_key].position)
  }

  pub fn normal(&self, verts: &SlotMap<VertexKey, Vertex>) -> Vector3<f32> {
    let [a, b, c] = self.vertex_positions(verts);
    (b - a).cross(&(c - a)).normalize()
  }

  pub fn area(&self, verts: &SlotMap<VertexKey, Vertex>) -> f32 {
    let [a, b, c] = self.vertex_positions(verts);
    0.5 * (b - a).cross(&(c - a)).magnitude()
  }

  pub fn center(&self, verts: &SlotMap<VertexKey, Vertex>) -> Vector3<f32> {
    let [a, b, c] = self.vertex_positions(verts);
    (a + b + c) / 3.
  }

  pub fn compute_angle_at_vertex(&self, vtx_ix: usize, verts: &SlotMap<VertexKey, Vertex>) -> f32 {
    let (target_vtx_key, b, c) = match vtx_ix {
      0 => (self.vertices[0], self.vertices[1], self.vertices[2]),
      1 => (self.vertices[1], self.vertices[2], self.vertices[0]),
      2 => (self.vertices[2], self.vertices[0], self.vertices[1]),
      _ => panic!("Vertex index {vtx_ix} out of bounds for triangle with 3 vertices"),
    };
    let [target_vtx_pos, b_vtx_pos, c_vtx_pos] = [
      verts[target_vtx_key].position,
      verts[b].position,
      verts[c].position,
    ];

    let edge_0 = b_vtx_pos - target_vtx_pos;
    let edge_1 = c_vtx_pos - target_vtx_pos;
    edge_0.angle(&edge_1)
  }

  pub fn compute_angle_at_vertex_key(
    &self,
    vtx_key: VertexKey,
    verts: &SlotMap<VertexKey, Vertex>,
  ) -> f32 {
    let vtx_ix = self
      .vertices
      .iter()
      .position(|&v| v == vtx_key)
      .unwrap_or_else(|| panic!("Vertex key {vtx_key:?} not found in face"));
    self.compute_angle_at_vertex(vtx_ix, verts)
  }

  pub fn to_triangle(&self, verts: &SlotMap<VertexKey, Vertex>) -> Triangle {
    Triangle {
      a: verts[self.vertices[0]].position,
      b: verts[self.vertices[1]].position,
      c: verts[self.vertices[2]].position,
    }
  }

  pub fn is_degenerate(&self, verts: &SlotMap<VertexKey, Vertex>) -> bool {
    self.to_triangle(verts).is_degenerate()
  }
}

#[derive(Clone, Debug)]
pub struct Edge {
  // Ordered such that the first vertex key is always less than the second
  pub vertices: [VertexKey; 2],
  pub faces: SmallVec<[FaceKey; 2]>,
  pub sharp: bool,
  pub displacement_normal: Option<Vec3>,
}

impl Edge {
  pub fn length(&self, verts: &SlotMap<VertexKey, Vertex>) -> f32 {
    let a = verts[self.vertices[0]].position;
    let b = verts[self.vertices[1]].position;
    (a - b).magnitude()
  }

  pub fn other_vtx(&self, vtx_key: VertexKey) -> VertexKey {
    if self.vertices[0] == vtx_key {
      self.vertices[1]
    } else if self.vertices[1] == vtx_key {
      self.vertices[0]
    } else {
      panic!("Vertex key {vtx_key:?} not found in edge");
    }
  }
}

fn distance(p0: Vec3, p1: Vec3) -> f32 {
  (p0 - p1).magnitude()
}

/// Mesh representation that maintains topological information between vertices,
/// edges, and faces.
///
/// Similar to a half-edge data structure, but with a some differences such as
/// all faces being triangles and no intrinsic directionality to edges.
/// Instead, faces store their vertices directly in-order.
#[derive(Clone, Default)]
pub struct LinkedMesh<FaceData = ()> {
  pub vertices: SlotMap<VertexKey, Vertex>,
  pub faces: SlotMap<FaceKey, Face<FaceData>>,
  pub edges: SlotMap<EdgeKey, Edge>,
  pub transform: Option<Mat4>,
}

impl<T> Debug for LinkedMesh<T> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
      f,
      "LinkedMesh {{ vertices: {}, faces: {}, edges: {} }}",
      self.vertices.len(),
      self.faces.len(),
      self.edges.len()
    )
  }
}

fn sort_edge(v0: VertexKey, v1: VertexKey) -> [VertexKey; 2] {
  let v0_ix = vkey_ix(&v0);
  let v1_ix = vkey_ix(&v1);
  if v0_ix < v1_ix {
    [v0, v1]
  } else {
    [v1, v0]
  }
}

fn sort_edge_inplace<const DENSE: bool>(vtxs: &mut [VertexKey; 2]) {
  if DENSE {
    let [vtx0, vtx1] = vtxs;
    debug_assert_eq!(vkey_version(vtx0), 1);
    debug_assert_eq!(vkey_version(vtx1), 1);
    let v0_ix = vkey_ix_mut(vtx0);
    let v1_ix = vkey_ix_mut(vtx1);

    if *v0_ix > *v1_ix {
      std::mem::swap(v0_ix, v1_ix);
    }
  } else {
    let v0_ix = vkey_ix(&vtxs[0]);
    let v1_ix = vkey_ix(&vtxs[1]);
    if v0_ix > v1_ix {
      vtxs.swap(0, 1);
    }
  }
}

#[derive(Debug)]
struct NormalAcc {
  pub accumulated_normal: Vec3,
}

impl NormalAcc {
  pub fn add_face<FaceData>(
    &mut self,
    fan_center_vtx_key: VertexKey,
    face_key: FaceKey,
    verts: &SlotMap<VertexKey, Vertex>,
    faces: &SlotMap<FaceKey, Face<FaceData>>,
  ) -> Option<Vec3> {
    let face = &faces[face_key];
    // Filter out sliver triangles that can destabilize the average normal.
    if face.area(verts) < EPSILON {
      return None;
    }
    let face_normal = face.normal(verts);
    if face_normal.x.is_nan() || face_normal.y.is_nan() || face_normal.z.is_nan() {
      // panic!(
      //   "Face normal is NaN: {:?}; is_degen={}",
      //   face.to_triangle(verts),
      //   face.is_degenerate(verts)
      // );
      log::warn!(
        "Face normal is NaN: {:?}; is_degen={}; face_key={face_key:?}",
        face.to_triangle(verts),
        face.is_degenerate(verts)
      );
      return None;
    }

    let angle_at_vtx = face.compute_angle_at_vertex_key(fan_center_vtx_key, verts);
    if angle_at_vtx.is_nan() {
      ::log::error!("NaN angle: {:?}", face.to_triangle(verts));
      return None;
    }

    let weighted_normal = face_normal * angle_at_vtx;
    self.accumulated_normal += weighted_normal;
    Some(weighted_normal)
  }

  pub fn get(&self) -> Option<Vec3> {
    if self.accumulated_normal == Vec3::zeros() {
      None
    } else {
      let normalized = self.accumulated_normal.normalize();
      if normalized.x.is_nan() || normalized.y.is_nan() || normalized.z.is_nan() {
        panic!("Non-zero but still NaN normalized: {self:?}");
      }
      Some(normalized)
    }
  }

  fn new() -> Self {
    Self {
      accumulated_normal: Vec3::zeros(),
    }
  }
}

struct SmoothFan {
  pub old_key: VertexKey,
  pub face_keys: SmallVec<[FaceKey; 8]>,
  pub normal: Vec3,
}

/// Determines the method used to compute displacement normals for new vertices
/// produced when splitting edges
#[repr(u32)]
#[derive(Clone, Copy)]
pub enum DisplacementNormalMethod {
  /// Displacement normals are generated by mixing the normals of the vertices
  /// of edges when splitting them
  Interpolate = 0,
  /// Displacement normals are assigned from the normal of the edge when
  /// splitting them
  EdgeNormal = 1,
}

impl TryFrom<u32> for DisplacementNormalMethod {
  type Error = ();

  fn try_from(value: u32) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(DisplacementNormalMethod::Interpolate),
      1 => Ok(DisplacementNormalMethod::EdgeNormal),
      _ => Err(()),
    }
  }
}

#[derive(Debug, Clone)]
pub enum NonManifoldError {
  /// No faces in the mesh
  EmptyMesh,
  /// Edge is not connected to any faces
  LooseEdge { edge_key: EdgeKey },
  /// Vertex is either not connected to any edges or not used in any faces
  LooseVertex { vtx_key: VertexKey },
  /// Edge is connected to more than two faces
  NonManifoldEdge {
    edge_key: EdgeKey,
    face_count: usize,
  },
  /// Vertex has more than one fan of faces around it
  MultipleFans {
    vtx_key: VertexKey,
    incident_face_count: usize,
    visited_face_count: usize,
  },
  /// The fan of faces around a vertex is not closed
  NonClosedFan { vtx_key: VertexKey },
}

pub struct EdgeSplitPos {
  /// Number from 0. to 1. representing the position along the edge from the
  /// starting vertex to split at
  pub pos: f32,
  pub start_vtx_key: VertexKey,
}

impl EdgeSplitPos {
  fn get(&self, v0_key: VertexKey) -> f32 {
    if v0_key == self.start_vtx_key {
      self.pos
    } else {
      1. - self.pos
    }
  }

  pub fn middle() -> Self {
    Self {
      pos: 0.5,
      start_vtx_key: VertexKey::null(),
    }
  }
}

const EPSILON: f32 = 1e-5;

#[derive(Clone, Debug)]
pub struct Plane {
  pub normal: Vec3,
  pub w: f32,
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

impl std::ops::BitOr for PolygonClass {
  type Output = Self;

  fn bitor(self, rhs: Self) -> Self::Output {
    let out = self as u8 | rhs as u8;
    out.into()
  }
}

/// Generates vertices arranged in rings at different heights.
///
/// Creates `height_segments + 1` rings, each with `radial_segments` vertices.
/// - `radius_fn(h)`: returns the radius at normalized height `h` (0.0 to 1.0)
/// - `y_fn(h)`: returns the y-coordinate at normalized height `h` (0.0 to 1.0)
fn generate_radial_vertices(
  vertices: &mut Vec<Vec3>,
  radial_segments: usize,
  height_segments: usize,
  radius_fn: impl Fn(f32) -> f32,
  y_fn: impl Fn(f32) -> f32,
) {
  for h in 0..=height_segments {
    let h_norm = h as f32 / height_segments as f32;
    let y = y_fn(h_norm);
    let radius = radius_fn(h_norm);
    for r in 0..radial_segments {
      let theta = (r as f32 / radial_segments as f32) * 2. * PI;
      let x = radius * theta.cos();
      let z = radius * theta.sin();
      vertices.push(Vec3::new(x, y, z));
    }
  }
}

/// Generates quad (two triangle) indices connecting adjacent height rings.
///
/// Connects rings of vertices arranged in the pattern created by `generate_radial_vertices`.
fn generate_radial_side_indices(
  indices: &mut Vec<u32>,
  radial_segments: usize,
  height_segments: usize,
) {
  for h in 0..height_segments {
    for r in 0..radial_segments {
      let v0 = h * radial_segments + r;
      let v1 = v0 + radial_segments;
      let next_r = (r + 1) % radial_segments;
      let v0_next = h * radial_segments + next_r;
      let v1_next = v0_next + radial_segments;

      indices.push(v0 as u32);
      indices.push(v1 as u32);
      indices.push(v0_next as u32);
      indices.push(v1 as u32);
      indices.push(v1_next as u32);
      indices.push(v0_next as u32);
    }
  }
}

/// Generates cap (fan) indices from a center vertex to a ring of vertices.
///
/// - `center_ix`: index of the center vertex
/// - `radial_segments`: number of vertices in the ring
/// - `ring_offset`: starting index of the ring vertices
/// - `reverse_winding`: if true, reverses the winding order (for top caps)
fn generate_cap_indices(
  indices: &mut Vec<u32>,
  center_ix: u32,
  radial_segments: usize,
  ring_offset: usize,
  reverse_winding: bool,
) {
  for i in 0..radial_segments {
    let curr = (ring_offset + i) as u32;
    let next = (ring_offset + ((i + 1) % radial_segments)) as u32;
    if reverse_winding {
      indices.push(next);
      indices.push(curr);
      indices.push(center_ix);
    } else {
      indices.push(center_ix);
      indices.push(curr);
      indices.push(next);
    }
  }
}

impl<FaceData: Default> LinkedMesh<FaceData> {
  pub fn new(vertex_count: usize, face_count: usize, transform: Option<Mat4>) -> Self {
    Self {
      vertices: SlotMap::with_capacity_and_key(vertex_count),
      faces: SlotMap::with_capacity_and_key(face_count),
      edges: SlotMap::with_key(),
      transform,
    }
  }

  pub fn iter_faces(&self) -> impl Iterator<Item = (FaceKey, &Face<FaceData>)> {
    self.faces.iter()
  }

  pub fn iter_edges(&self) -> impl Iterator<Item = (EdgeKey, &Edge)> {
    self.edges.iter()
  }

  pub fn iter_vertices(&self) -> impl Iterator<Item = (VertexKey, &Vertex)> {
    self.vertices.iter()
  }

  pub fn from_indexed_vertices(
    vertices: &[Vec3],
    indices: &[u32],
    normals: Option<&[Vec3]>,
    transform: Option<Mat4>,
  ) -> Self {
    if indices.len() % 3 != 0 {
      panic!(
        "Indices length must be a multiple of 3; got {}",
        indices.len()
      );
    }

    let vertices: SlotMap<VertexKey, Vertex> =
      build_slotmap_from_iter(vertices.iter().enumerate().map(|(i, &vtx_pos)| Vertex {
        position: vtx_pos,
        shading_normal: normals.map(|normals| normals[i]),
        displacement_normal: None,
        edges: SmallVec::new(),
        _padding: Default::default(),
      }));

    let temp_faces: LocalSlotMap<FaceKey, Face<()>> = LocalSlotMap {
      slots: Vec::new(),
      free_head: 1,
      num_elems: 0,
      _k: PhantomData,
    };
    let mut mesh = Self {
      vertices,
      faces: unsafe { std::mem::transmute(temp_faces) },
      edges: SlotMap::with_key(),
      transform,
    };

    mesh.faces = build_slotmap_from_iter_with_key(
      indices.as_chunks::<3>().0.iter(),
      |[a_ix, b_ix, c_ix], face_key| {
        // we've inserted all the vertices in a fixed order without any removals, so the keys are
        // deterministic
        let a = vkey(a_ix + 1, 1);
        let b = vkey(b_ix + 1, 1);
        let c = vkey(c_ix + 1, 1);
        let vertices = [a, b, c];

        let edges = [(0, 1), (1, 2), (2, 0)];
        let face = Face {
          vertices,
          edges: edges.map(|(a, b)| {
            let edge_key = mesh.get_or_create_edge::<true>([vertices[a], vertices[b]]);

            let edge = unsafe { mesh.edges.get_unchecked_mut(edge_key) };
            edge.faces.push(face_key);

            unsafe {
              std::mem::transmute(LocalKeyData {
                idx: ekey_ix(&edge_key),
                version: NonZeroU32::new_unchecked(1),
              })
            }
          }),
          data: Default::default(),
        };

        face
      },
    );

    mesh
  }

  pub fn from_triangles(triangles: impl IntoIterator<Item = Triangle>) -> Self {
    let triangles = triangles.into_iter();
    let (min_size, max_size) = triangles.size_hint();
    let size = max_size.unwrap_or(min_size);
    let mut mesh = Self::new(size * 3, size, None);

    // TODO: optimize using fastpath slotmap methods
    for tri in triangles {
      // This might break mesh topology in some cases, but it saves us from dealing
      // with NaNs
      if tri.is_degenerate() {
        log::warn!("Skipping degenerate triangle: {tri:?}");
        continue;
      }

      let [a_key, b_key, c_key] = [
        mesh.vertices.insert(Vertex {
          position: tri.a,
          shading_normal: None,
          displacement_normal: None,
          edges: SmallVec::new(),
          _padding: Default::default(),
        }),
        mesh.vertices.insert(Vertex {
          position: tri.b,
          shading_normal: None,
          displacement_normal: None,
          edges: SmallVec::new(),
          _padding: Default::default(),
        }),
        mesh.vertices.insert(Vertex {
          position: tri.c,
          shading_normal: None,
          displacement_normal: None,
          edges: SmallVec::new(),
          _padding: Default::default(),
        }),
      ];

      mesh.add_face::<true>([a_key, b_key, c_key], Default::default());
    }

    mesh
  }

  /// Removes `v1` and updates all references to it to point to `v0` instead.
  pub fn merge_vertices(&mut self, v0_key: VertexKey, v1_key: VertexKey) {
    let removed_vtx = self.vertices.remove(v1_key).unwrap_or_else(|| {
      panic!(
        "Tried to merge vertex that doesn't exist; key={v1_key:?}. Was referenced by removed \
         vertex with key={v0_key:?}",
      )
    });

    for &edge_key in &removed_vtx.edges {
      let (pair_vtx_key, new_edge_vertices) = {
        let edge = &mut self.edges[edge_key];
        for &face_key in &edge.faces {
          let face = &mut self.faces[face_key];
          if face.vertices.contains(&v0_key) && face.vertices.contains(&v1_key) {
            let v0 = &self.vertices[v0_key];
            panic!(
              "Triangle {face_key:?} contains both vertices to merge: {v0_key:?}, {v1_key:?} with \
               positions {} and {}",
              v0.position, removed_vtx.position
            );
          }
          for vert_key in &mut face.vertices {
            if *vert_key == v1_key {
              *vert_key = v0_key;
            }
          }
        }

        let pair_vtx_key = if edge.vertices[0] == v1_key {
          edge.vertices[0] = v0_key;
          edge.vertices[1]
        } else if edge.vertices[1] == v1_key {
          edge.vertices[1] = v0_key;
          edge.vertices[0]
        } else {
          panic!("Edge {edge_key:?} doesn't contain vertex {v1_key:?} to merge",)
        };

        let v0 = &mut self.vertices[v0_key];
        v0.edges.push(edge_key);

        sort_edge_inplace::<false>(&mut edge.vertices);
        (pair_vtx_key, edge.vertices)
      };

      // We've updated the edge between (v1 and v_pair) to be between (v0 and v_pair)
      //
      // However, it's possible that there's already an edge between v0 and v_pair.
      // If that's the case, we merge this edge with that one and drop it from the
      // graph.
      let pair_vtx = &self.vertices[pair_vtx_key];
      let mut edge_key_to_merge_into = None;
      for &pair_edge_key in &pair_vtx.edges {
        if pair_edge_key == edge_key {
          continue;
        }

        let pair_edge = &self.edges[pair_edge_key];
        if pair_edge.vertices == new_edge_vertices {
          if edge_key_to_merge_into.is_some() {
            panic!(
              "Multiple edges found to merge into; \
               edge_key_to_merge_into={edge_key_to_merge_into:?}; pair_edge_key={pair_edge_key:?}",
            );
          }
          edge_key_to_merge_into = Some(pair_edge_key);
        }
      }

      if let Some(edge_key_to_merge_into) = edge_key_to_merge_into {
        let dropped_edge = self.edges.remove(edge_key).unwrap();
        let merged_into_edge = &mut self.edges[edge_key_to_merge_into];

        for &face_key in &dropped_edge.faces {
          let face = &mut self.faces[face_key];
          for o_edge_key in &mut face.edges {
            if *o_edge_key == edge_key {
              *o_edge_key = edge_key_to_merge_into;
            }
          }

          if !merged_into_edge.faces.contains(&face_key) {
            merged_into_edge.faces.push(face_key);
          }
        }

        for &vert_key in &dropped_edge.vertices {
          let vert = &mut self.vertices[vert_key];
          swap_retain_sv(&mut vert.edges, |&mut e| e != edge_key);
        }
      }
    }

    if cfg!(debug_assertions) {
      let mut uniq_edges = FxHashSet::default();
      for (_edge_key, edge) in &self.edges {
        if edge.vertices.contains(&v1_key) {
          panic!("Edge still contains removed vertex; edge={edge:?}; v0={v0_key:?}; v1={v1_key:?}",);
        }

        if !uniq_edges.insert(edge.vertices) {
          let dupe_edges = self
            .edges
            .iter()
            .filter(|(_, e)| e.vertices == edge.vertices)
            .collect::<Vec<_>>();
          let removed_vtx_edges_after = removed_vtx
            .edges
            .iter()
            .map(|&k| &self.edges[k])
            .collect::<Vec<_>>();
          graphviz_print(&self.debug());
          panic!(
            "Duplicate edge found after merging vertices removed_edge={edge:?};\n \
             dupe_edges={dupe_edges:?};\nv0={v0_key:?};\n v1={v1_key:?};\n \
             removed_vtx_edges_after={removed_vtx_edges_after:?}"
          );
        }
      }
    }
  }

  // TODO: The spatial bucketing used here could maybe be replaced with the `rstar` crate
  // https://docs.rs/rstar/latest/rstar/
  pub fn merge_vertices_by_distance_cb(
    &mut self,
    max_distance: f32,
    mut cb: impl FnMut(FaceKey, FaceData),
  ) -> usize {
    // simple spatial hashing
    // TODO: this seems trash and should be re-written to use a proper r-tree or similar
    let buckets_per_dim = 32;
    let mut buckets: FxHashMap<usize, Vec<_>> = FxHashMap::default();

    let (mut mins, mut maxs) = self.vertices.values().fold(
      (
        Vec3::new(f32::MAX, f32::MAX, f32::MAX),
        Vec3::new(f32::MIN, f32::MIN, f32::MIN),
      ),
      |(mins, maxs), vtx| (mins.inf(&vtx.position), maxs.sup(&vtx.position)),
    );
    // expand bounds and offset center slightly to allow more chances to avoid
    // searching neighboring buckets
    mins -= Vec3::new(0.3828593, 0.2859821, 0.18533925);
    maxs += Vec3::new(0.3828593, 0.2859821, 0.18533925);

    let x_range = maxs.x - mins.x;
    let y_range = maxs.y - mins.y;
    let z_range = maxs.z - mins.z;

    let get_bucket_ix = |x_ix: usize, y_ix: usize, z_ix: usize| -> usize {
      x_ix + y_ix * buckets_per_dim + z_ix * buckets_per_dim * buckets_per_dim
    };

    let get_bucket_coords = |pos: Vec3| -> ([usize; 3], bool) {
      let x_ix = ((pos.x - mins.x) / x_range * buckets_per_dim as f32).floor() as usize;
      let y_ix = ((pos.y - mins.y) / y_range * buckets_per_dim as f32).floor() as usize;
      let z_ix = ((pos.z - mins.z) / z_range * buckets_per_dim as f32).floor() as usize;

      // we can completely skip searching neighboring buckets if the distance between
      // this point and all of the bucket boundaries is greater than the max_distance
      let needs_neighbor_search = 'b: {
        let [bucket_mins_x, bucket_maxs_x] = [
          mins.x + x_ix as f32 / buckets_per_dim as f32 * x_range,
          mins.x + (x_ix + 1) as f32 / buckets_per_dim as f32 * x_range,
        ];
        if pos.x - bucket_mins_x < max_distance || bucket_maxs_x - pos.x < max_distance {
          break 'b true;
        }

        let [bucket_mins_y, bucket_maxs_y] = [
          mins.y + y_ix as f32 / buckets_per_dim as f32 * y_range,
          mins.y + (y_ix + 1) as f32 / buckets_per_dim as f32 * y_range,
        ];
        if pos.y - bucket_mins_y < max_distance || bucket_maxs_y - pos.y < max_distance {
          break 'b true;
        }

        let [bucket_mins_z, bucket_maxs_z] = [
          mins.z + z_ix as f32 / buckets_per_dim as f32 * z_range,
          mins.z + (z_ix + 1) as f32 / buckets_per_dim as f32 * z_range,
        ];
        if pos.z - bucket_mins_z < max_distance || bucket_maxs_z - pos.z < max_distance {
          break 'b true;
        }

        false
      };

      ([x_ix, y_ix, z_ix], needs_neighbor_search)
    };

    for (vtx_key, vtx) in &self.vertices {
      let (bucket_coords, _) = get_bucket_coords(vtx.position);
      let bucket_ix = get_bucket_ix(bucket_coords[0], bucket_coords[1], bucket_coords[2]);
      buckets
        .entry(bucket_ix)
        .or_insert_with(Vec::new)
        .push(vtx_key);
    }

    let mut removed_vert_keys = FxHashSet::default();
    let all_vert_keys = self.vertices.keys().collect::<Vec<_>>();

    let mut vertices_to_merge = Vec::new();
    for vtx_key in all_vert_keys {
      if removed_vert_keys.contains(&vtx_key) {
        continue;
      }

      let vtx = &self.vertices[vtx_key];
      let (bucket_coords, needs_neighbor_search) = get_bucket_coords(vtx.position);
      let bucket_ix = get_bucket_ix(bucket_coords[0], bucket_coords[1], bucket_coords[2]);

      let bucket = buckets.get_mut(&bucket_ix).unwrap();
      let mut o_vtx_ix = 0usize;
      while o_vtx_ix < bucket.len() {
        let o_vtx_key = bucket[o_vtx_ix];
        if o_vtx_key == vtx_key {
          o_vtx_ix += 1;
          continue;
        }

        let o_vtx = &self.vertices[o_vtx_key];
        if distance(vtx.position, o_vtx.position) < max_distance {
          vertices_to_merge.push(o_vtx_key);
          bucket.swap_remove(o_vtx_ix);
        } else {
          o_vtx_ix += 1;
        }
      }

      if needs_neighbor_search {
        // this is a bit lazy but this codepath should be rare enough (when using
        // reasonably small merge distances) that it won't matter
        for x_ix in 0..buckets_per_dim {
          if ((x_ix as isize) - bucket_coords[0] as isize).abs() > 1 {
            continue;
          }
          for y_ix in 0..buckets_per_dim {
            if ((y_ix as isize) - bucket_coords[1] as isize).abs() > 1 {
              continue;
            }
            for z_ix in 0..buckets_per_dim {
              if ((z_ix as isize) - bucket_coords[2] as isize).abs() > 1 {
                continue;
              }

              let neighbor_bucket_ix = get_bucket_ix(x_ix, y_ix, z_ix);
              let Some(bucket) = buckets.get_mut(&neighbor_bucket_ix) else {
                continue;
              };

              let mut i = 0;
              while i < bucket.len() {
                let o_vtx_key = bucket[i];
                if o_vtx_key == vtx_key {
                  i += 1;
                  continue;
                }

                let o_vtx = &self.vertices[o_vtx_key];
                if distance(vtx.position, o_vtx.position) < max_distance {
                  vertices_to_merge.push(o_vtx_key);
                  bucket.swap_remove(i);
                } else {
                  i += 1;
                }
              }
            }
          }
        }
      }

      for o_vtx_key in vertices_to_merge.drain(..) {
        // for every face that contains both vertices to merge, we remove it
        let mut face_keys_to_remove = Vec::new();
        for &edge_key in &self.vertices[o_vtx_key].edges {
          let edge = &self.edges[edge_key];
          for &face_key in &edge.faces {
            let face = &self.faces[face_key];
            if face.vertices.contains(&vtx_key) {
              face_keys_to_remove.push(face_key);
            }
          }
        }
        if !face_keys_to_remove.is_empty() {
          for face_key in face_keys_to_remove {
            // face might have been removed already
            if self.faces.contains_key(face_key) {
              let face_data = self.remove_face(face_key);
              cb(face_key, face_data);
            }
          }
        }

        self.merge_vertices(vtx_key, o_vtx_key);
        removed_vert_keys.insert(o_vtx_key);
      }
    }

    removed_vert_keys.len()
  }

  pub fn merge_vertices_by_distance(&mut self, max_distance: f32) -> usize {
    self.merge_vertices_by_distance_cb(max_distance, |_, _| ())
  }

  pub fn debug(&self) -> String {
    fn format_vtx(key: VertexKey, _vtx: &Vertex) -> String {
      format!("{key:?}",)
    }

    fn format_edge(key: EdgeKey, edge: &Edge) -> String {
      format!("{key:?} {:?} -> {:?}", edge.vertices[0], edge.vertices[1])
    }

    fn format_face<FaceData>(key: FaceKey, face: &Face<FaceData>) -> String {
      format!(
        "{key:?} {:?} -> {:?} -> {:?}",
        face.vertices[0], face.vertices[1], face.vertices[2]
      )
    }

    let mut connections = Vec::new();
    for (vtx_key, vtx) in self.iter_vertices() {
      let vtx_name = format_vtx(vtx_key, vtx);

      for &edge_key in &vtx.edges {
        let edge = &self.edges.get(edge_key).unwrap_or_else(|| {
          panic!(
            "Tried to get edge that doesn't exist; key={edge_key:?}. Was referenced by vertex \
             with key={vtx_key:?}",
          )
        });
        let edge = format_edge(edge_key, edge);
        connections.push((vtx_name.clone(), edge));
      }
    }

    for (edge_key, edge) in self.iter_edges() {
      let edge_name = format_edge(edge_key, edge);

      for &face_key in &edge.faces {
        let face = &self.faces[face_key];
        let face_name = format_face(face_key, face);
        connections.push((edge_name.clone(), face_name));
      }

      for &vtx_key in &edge.vertices {
        let vtx_name = self
          .vertices
          .get(vtx_key)
          .map(|vtx| format_vtx(vtx_key, vtx))
          .unwrap_or_else(|| {
            panic!(
              "Tried to get vertex that doesn't exist; key={vtx_key:?}. Was referenced by edge \
               with key={edge_key:?}"
            )
          });
        connections.push((edge_name.clone(), vtx_name));
      }
    }

    for (face_key, face) in self.iter_faces() {
      let face_name = format_face(face_key, face);

      for &edge_key in &face.edges {
        let edge = &self.edges[edge_key];
        let edge_name = format_edge(edge_key, edge);
        connections.push((face_name.clone(), edge_name));
      }

      for &vtx_key in &face.vertices {
        let vtx = &self.vertices[vtx_key];
        let vtx_name = format_vtx(vtx_key, vtx);
        connections.push((face_name.clone(), vtx_name));
      }
    }

    connections
      .into_iter()
      .map(|(a, b)| format!("{a}::{b}"))
      .collect::<Vec<_>>()
      .join("\n")
  }

  pub fn from_raw_indexed(
    vertices: &[f32],
    indices: &[u32],
    normals: Option<&[f32]>,
    transform: Option<Mat4>,
  ) -> Self {
    assert!(
      vertices.len() % 3 == 0,
      "Vertices length must be a multiple of 3; got {}",
      vertices.len()
    );
    let vertices =
      unsafe { std::slice::from_raw_parts(vertices.as_ptr() as *const Vec3, vertices.len() / 3) };
    let normals = if let Some(normals) = normals {
      if normals.is_empty() {
        None
      } else {
        assert!(
          normals.len() % 3 == 0,
          "Normals length must be a multiple of 3; got {}",
          normals.len()
        );
        Some(unsafe {
          std::slice::from_raw_parts(normals.as_ptr() as *const Vec3, normals.len() / 3)
        })
      }
    } else {
      None
    };

    Self::from_indexed_vertices(vertices, indices, normals, transform)
  }

  fn replace_vertex_in_face(
    &mut self,
    face_key: FaceKey,
    old_vtx_key: VertexKey,
    new_vtx_key: VertexKey,
  ) {
    let (
      edge_indices_to_alter,
      old_edge_keys,
      pair_vtx_keys,
      edge_displacement_normals,
      edge_sharpness,
    ) = {
      let face = &mut self.faces[face_key];
      let mut edge_indices_to_alter: [usize; 2] = [usize::MAX, usize::MAX];
      let mut pair_vtx_keys: [VertexKey; 2] = [VertexKey::null(), VertexKey::null()];
      let mut edge_displacement_normals: [Option<Vec3>; 2] = [None, None];
      let mut edge_sharpness: [bool; 2] = [false, false];
      let mut old_edge_keys: [EdgeKey; 2] = [EdgeKey::null(), EdgeKey::null()];

      for (edge_ix, &edge_key) in face.edges.iter().enumerate() {
        let edge = &self.edges[edge_key];
        if edge.vertices.contains(&old_vtx_key) {
          if edge_indices_to_alter[0] == usize::MAX {
            edge_indices_to_alter[0] = edge_ix;
            old_edge_keys[0] = edge_key;
            pair_vtx_keys[0] = if edge.vertices[0] == old_vtx_key {
              edge.vertices[1]
            } else {
              edge.vertices[0]
            };
            edge_displacement_normals[0] = edge.displacement_normal;
            edge_sharpness[0] = edge.sharp;
          } else {
            edge_indices_to_alter[1] = edge_ix;
            old_edge_keys[1] = edge_key;
            pair_vtx_keys[1] = if edge.vertices[0] == old_vtx_key {
              edge.vertices[1]
            } else {
              edge.vertices[0]
            };
            edge_displacement_normals[1] = edge.displacement_normal;
            edge_sharpness[1] = edge.sharp;
            break;
          }
        }
      }

      (
        edge_indices_to_alter,
        old_edge_keys,
        pair_vtx_keys,
        edge_displacement_normals,
        edge_sharpness,
      )
    };

    // remove this face from the old edges and delete them if they no longer
    // reference any faces
    for old_edge_key in old_edge_keys {
      let edge = &mut self.edges[old_edge_key];
      swap_retain_sv(&mut edge.faces, |&mut f| f != face_key);

      if edge.faces.is_empty() {
        for vtx_key in edge.vertices {
          let vtx = &mut self.vertices[vtx_key];
          swap_retain_sv(&mut vtx.edges, |&mut e| e != old_edge_key);
        }
        self.edges.remove(old_edge_key);
      }
    }

    let new_edge_key_0 = self.get_or_create_edge::<false>([new_vtx_key, pair_vtx_keys[0]]);
    let new_edge_key_1 = self.get_or_create_edge::<false>([new_vtx_key, pair_vtx_keys[1]]);
    self.edges[new_edge_key_0].faces.push(face_key);
    self.edges[new_edge_key_0].displacement_normal = edge_displacement_normals[0];
    self.edges[new_edge_key_0].sharp = edge_sharpness[0];
    self.edges[new_edge_key_1].faces.push(face_key);
    self.edges[new_edge_key_1].displacement_normal = edge_displacement_normals[1];
    self.edges[new_edge_key_1].sharp = edge_sharpness[1];

    let face = &mut self.faces[face_key];

    // update the edge keys
    face.edges[edge_indices_to_alter[0]] = new_edge_key_0;
    face.edges[edge_indices_to_alter[1]] = new_edge_key_1;

    // actually update the vertex key in the face
    let vtx_key_ix_to_alter = face
      .vertices
      .iter()
      .position(|&v| v == old_vtx_key)
      .unwrap();
    face.vertices[vtx_key_ix_to_alter] = new_vtx_key;
  }

  /// This check assumes that the mesh consists of a single connected component.  If there are
  /// islands or disconnected parts, this may produce incorrect results.
  ///
  /// If `TWO_MANIFOLD` is `true`, additionally enforces that every edge is shared by *exactly* two
  /// faces, meaning that the mesh is watertight and forms a continuous surface.
  ///
  /// The test is entirely topological; positions, normals, triangle winding order, etc. are not
  /// checked.
  pub fn check_is_manifold<const TWO_MANIFOLD: bool>(&self) -> Result<(), NonManifoldError> {
    for (edge_key, edge) in self.edges.iter() {
      let face_count = edge.faces.len();
      if TWO_MANIFOLD {
        if face_count != 2 {
          return Err(NonManifoldError::NonManifoldEdge {
            edge_key,
            face_count,
          });
        }
      } else {
        if face_count == 0 {
          log::error!("Found edge with no faces: {edge:?}");
          return Err(NonManifoldError::LooseEdge { edge_key });
        } else if face_count > 2 {
          return Err(NonManifoldError::NonManifoldEdge {
            edge_key,
            face_count,
          });
        }
      }
    }

    if self.faces.is_empty() {
      return Err(NonManifoldError::EmptyMesh);
    }

    // For each vertex, walk the fan of incident faces
    let mut incident_faces = Vec::new();
    let mut visited_faces = FxHashSet::default();
    let mut visited_edges = FxHashSet::default();
    for (vtx_key, vtx) in &self.vertices {
      if vtx.edges.is_empty() {
        return Err(NonManifoldError::LooseVertex { vtx_key });
      }

      incident_faces.clear();
      for &edge_key in &vtx.edges {
        incident_faces.extend(self.edges[edge_key].faces.iter().copied());
      }
      incident_faces.sort_unstable();
      incident_faces.dedup();
      let Some(&start_face_key) = incident_faces.first() else {
        return Err(NonManifoldError::LooseVertex { vtx_key });
      };
      let start_face = &self.faces[start_face_key];

      visited_faces.clear();
      visited_edges.clear();

      // Find an edge of the face that contains this vertex.  Prefer starting on an edge with only
      // one indicent face if one can be found so that we don't start in the middle of a non-closed
      // fan, but then pick any edge if the fan is closed as we'll end up walking all the way around
      let start_edge_key = start_face
        .edges
        .iter()
        .find(|e| {
          let edge = &self.edges[**e];
          edge.vertices.contains(&vtx_key) && edge.faces.len() == 1
        })
        .copied()
        .unwrap_or_else(|| {
          start_face
            .edges
            .iter()
            .find(|&&e| self.edges[e].vertices.contains(&vtx_key))
            .copied()
            .unwrap()
        });
      let mut cur_face_key = start_face_key;
      let mut cur_edge_key = start_edge_key;
      let mut full_fan = false;
      loop {
        visited_faces.insert(cur_face_key);
        visited_edges.insert(cur_edge_key);
        // choose the next edge in the current face that contains this vertex and is not the
        // previous edge
        let face = &self.faces[cur_face_key];
        let next_edge_key = face
          .edges
          .iter()
          .find(|&&edge_key| {
            self.edges[edge_key].vertices.contains(&vtx_key) && edge_key != cur_edge_key
          })
          .copied();

        let Some(next_edge_key) = next_edge_key else {
          // reached a border edge
          break;
        };

        // Find the other face (besides cur_face_key) that shares this edge and contains the vertex
        let edge = &self.edges[next_edge_key];
        let Some(&next_face_key) = edge.faces.iter().find(|&&face_key| {
          face_key != cur_face_key && self.faces[face_key].vertices.contains(&vtx_key)
        }) else {
          // reached a border edge
          break;
        };

        if visited_faces.contains(&next_face_key) {
          full_fan = true;
          break;
        }
        cur_face_key = next_face_key;
        cur_edge_key = next_edge_key;
      }

      if visited_faces.len() != incident_faces.len() {
        return Err(NonManifoldError::MultipleFans {
          vtx_key,
          incident_face_count: incident_faces.len(),
          visited_face_count: visited_faces.len(),
        });
      }

      if TWO_MANIFOLD {
        if !full_fan {
          return Err(NonManifoldError::NonClosedFan { vtx_key });
        }
      }
    }

    Ok(())
  }

  /// Returns `true` when the mesh is a single connected component.  This check assumes that the
  /// mesh consists of a single connected component.  If there are islands or disconnected parts,
  /// this may produce incorrect results.
  ///
  /// If `TWO_MANIFOLD` is `true`, additionally enforces that every edge is shared by *exactly* two
  /// faces, meaning that the mesh is watertight and forms a continuous surface.
  ///
  /// The test is entirely topological; positions, normals, triangle winding order, etc. are not
  /// checked.
  pub fn check_is_manifold_dynamic(&self, two_manifold: bool) -> Result<(), NonManifoldError> {
    for (edge_key, edge) in self.edges.iter() {
      let face_count = edge.faces.len();
      if two_manifold {
        if face_count != 2 {
          return Err(NonManifoldError::NonManifoldEdge {
            edge_key,
            face_count,
          });
        }
      } else {
        if face_count == 0 {
          log::error!("Found edge with no faces: {edge:?}");
          return Err(NonManifoldError::LooseEdge { edge_key });
        } else if face_count > 2 {
          return Err(NonManifoldError::NonManifoldEdge {
            edge_key,
            face_count,
          });
        }
      }
    }

    if self.faces.is_empty() {
      return Err(NonManifoldError::EmptyMesh);
    }

    // For each vertex, walk the fan of incident faces
    let mut incident_faces = Vec::new();
    let mut visited_faces = FxHashSet::default();
    let mut visited_edges = FxHashSet::default();
    for (vtx_key, vtx) in &self.vertices {
      if vtx.edges.is_empty() {
        return Err(NonManifoldError::LooseVertex { vtx_key });
      }

      incident_faces.clear();
      for &edge_key in &vtx.edges {
        incident_faces.extend(self.edges[edge_key].faces.iter().copied());
      }
      incident_faces.sort_unstable();
      incident_faces.dedup();
      let Some(&start_face_key) = incident_faces.first() else {
        return Err(NonManifoldError::LooseVertex { vtx_key });
      };
      let start_face = &self.faces[start_face_key];

      visited_faces.clear();
      visited_edges.clear();

      // Find an edge of the face that contains this vertex
      let start_edge_key = start_face
        .edges
        .iter()
        .find(|&&e| self.edges[e].vertices.contains(&vtx_key))
        .copied()
        .unwrap();
      let mut cur_face_key = start_face_key;
      let mut cur_edge_key = start_edge_key;
      let mut full_fan = false;
      loop {
        visited_faces.insert(cur_face_key);
        visited_edges.insert(cur_edge_key);
        // choose the next edge in the current face that contains this vertex and is not the
        // previous edge
        let face = &self.faces[cur_face_key];
        let next_edge_key = face
          .edges
          .iter()
          .find(|&&edge_key| {
            self.edges[edge_key].vertices.contains(&vtx_key) && edge_key != cur_edge_key
          })
          .copied();
        if next_edge_key.is_none() {
          break;
        }

        let next_edge_key = next_edge_key.unwrap();
        // Find the other face (besides cur_face_key) that shares this edge and contains the vertex
        let edge = &self.edges[next_edge_key];
        let Some(&next_face_key) = edge.faces.iter().find(|&&face_key| {
          face_key != cur_face_key && self.faces[face_key].vertices.contains(&vtx_key)
        }) else {
          // reached a border edge
          break;
        };

        if visited_faces.contains(&next_face_key) {
          full_fan = true;
          break;
        }
        cur_face_key = next_face_key;
        cur_edge_key = next_edge_key;
      }

      if visited_faces.len() != incident_faces.len() {
        return Err(NonManifoldError::MultipleFans {
          vtx_key,
          incident_face_count: incident_faces.len(),
          visited_face_count: visited_faces.len(),
        });
      }

      if two_manifold {
        if !full_fan {
          return Err(NonManifoldError::NonClosedFan { vtx_key });
        }
      }
    }

    Ok(())
  }

  pub fn compute_edge_displacement_normals(&mut self) {
    for edge in self.edges.values_mut() {
      let edge_displacement_normal = edge
        .faces
        .iter()
        .map(|&face_key| {
          let face = &self.faces[face_key];
          face.normal(&self.vertices)
        })
        .sum::<Vec3>()
        .normalize();
      edge.displacement_normal = Some(edge_displacement_normal);
    }
  }

  pub fn mark_edge_sharpness(&mut self, sharp_edge_threshold_rads: f32) {
    for edge in self.edges.values_mut() {
      // If edge has explicitly been marked as sharp, don't change it
      if edge.sharp {
        continue;
      }

      // Border edges as well as edges belonging to more than 3 faces are
      // automatically sharp
      if edge.faces.len() != 2 {
        edge.sharp = true;
        continue;
      }

      let [face0, face1] = [edge.faces[0], edge.faces[1]];
      let [normal0, normal1] = [
        self.faces[face0].normal(&self.vertices),
        self.faces[face1].normal(&self.vertices),
      ];
      let angle = normal0.angle(&normal1);
      edge.sharp = angle > sharp_edge_threshold_rads;
    }
  }

  /// Accumulates faces that are part of a single smooth fan around `vtx_key`
  /// into `smooth_fan_faces`.
  ///
  /// Returns the normal of the smooth fan on `vtx_key`, weighted by the angle
  /// between the each face's edges that meet at `vtx_key`
  fn walk_one_smooth_fan(
    &mut self,
    vtx_key: VertexKey,
    visited_edges: &mut BitSlice,
    visited_faces: &mut SmallVec<[FaceKey; 16]>,
    smooth_fan_faces: &mut SmallVec<[FaceKey; 16]>,
    vtx_normal_acc: &mut NormalAcc,
  ) -> Option<Vec3> {
    let vtx = &self.vertices[vtx_key];

    let mut fan_normal_acc = NormalAcc::new();

    // Walks around the smooth fan in one direction, recording visited edges and
    // faces into the respective buffers, until a sharp or border edge is
    // encountered.
    let walk = |visited_edges: &mut BitSlice,
                visited_faces: &mut SmallVec<[FaceKey; 16]>,
                smooth_fan_faces: &mut SmallVec<[FaceKey; 16]>,
                cur_edge_key: &mut EdgeKey,
                cur_face_key: &mut FaceKey,
                fan_normal_acc: &mut NormalAcc,
                vtx_normal_acc: &mut NormalAcc| loop {
      smooth_fan_faces.push(*cur_face_key);
      visited_faces.push(*cur_face_key);
      let weighted_normal =
        fan_normal_acc.add_face(vtx_key, *cur_face_key, &self.vertices, &self.faces);
      if let Some(weighted_normal) = weighted_normal {
        vtx_normal_acc.accumulated_normal += weighted_normal;
      }
      let cur_edge_ix = vtx.edges.iter().position(|&e| e == *cur_edge_key).unwrap();
      visited_edges.set(cur_edge_ix, true);

      // Try to walk to the next face in the smooth fan that shares the current edge
      let next_edge_key = self.faces[*cur_face_key]
        .edges
        .iter()
        .find(|&&edge_key| edge_key != *cur_edge_key && vtx.edges.contains(&edge_key));
      let (next_edge_key, next_edge) = match next_edge_key {
        Some(&edge_key) => (edge_key, &self.edges[edge_key]),
        None => {
          // We've reached the end of the smooth fan
          break;
        }
      };
      let next_edge_ix = vtx.edges.iter().position(|&e| e == next_edge_key).unwrap();
      if visited_edges[next_edge_ix] {
        break;
      }

      let next_face_key = next_edge
        .faces
        .iter()
        .find(|&&face_key| face_key != *cur_face_key && !visited_faces.contains(&face_key));
      let Some(&next_face_key) = next_face_key else {
        // This edge is a border edge
        visited_edges.set(next_edge_ix, true);
        break;
      };

      if next_edge.sharp {
        // We've hit a sharp edge and can stop walking
        break;
      }

      *cur_edge_key = next_edge_key;
      *cur_face_key = next_face_key;
    };

    let start_edge_ix = visited_edges.iter().position(|visited| !*visited);
    let Some(start_edge_ix) = start_edge_ix else {
      // We've visited all edges
      return fan_normal_acc.get();
    };

    let start_edge_key = vtx.edges[start_edge_ix];
    let start_face_key = self.edges[start_edge_key].faces.iter().find(|&&face_key| {
      if visited_faces.contains(&face_key) {
        return false;
      }

      let face = &self.faces[face_key];
      // Find the edges that include the vertex
      let [mut edge_key_0, mut edge_key_1] = [EdgeKey::null(), EdgeKey::null()];
      for &edge_key in &face.edges {
        if !self.edges[edge_key].vertices.contains(&vtx_key) {
          continue;
        }
        if edge_key_0.is_null() {
          edge_key_0 = edge_key
        } else {
          edge_key_1 = edge_key;
          break;
        }
      }

      // One edge must be not visited in order for us to walk from it
      [edge_key_0, edge_key_1].into_iter().any(|edge_key| {
        // TODO: why is this necessary
        let Some(edge_key_ix) = vtx.edges.iter().position(|&e| e == edge_key) else {
          return false;
        };
        !visited_edges[edge_key_ix]
      })
    });
    let Some(&start_face_key) = start_face_key else {
      return fan_normal_acc.get();
    };

    // If the starting edge is smooth, we have to walk the other way once we hit a
    // sharp edge.
    let needs_walk_the_other_way = !self.edges[start_edge_key].sharp;

    let mut cur_edge_key = start_edge_key;
    let mut cur_face_key = start_face_key;

    walk(
      visited_edges,
      visited_faces,
      smooth_fan_faces,
      &mut cur_edge_key,
      &mut cur_face_key,
      &mut fan_normal_acc,
      vtx_normal_acc,
    );

    if !needs_walk_the_other_way {
      return fan_normal_acc.get();
    }

    let first_other_way_face_key = self.edges[start_edge_key].faces.iter().find(|&&face_key| {
      let face = &self.faces[face_key];
      face.vertices.contains(&vtx_key) && !visited_faces.contains(&face_key)
    });
    let Some(first_other_way_face_key) = first_other_way_face_key else {
      return fan_normal_acc.get();
    };

    cur_face_key = *first_other_way_face_key;
    cur_edge_key = start_edge_key;

    walk(
      visited_edges,
      visited_faces,
      smooth_fan_faces,
      &mut cur_edge_key,
      &mut cur_face_key,
      &mut fan_normal_acc,
      vtx_normal_acc,
    );

    fan_normal_acc.get()
  }

  /// Returns the normal of the full vertex, including all smooth fans.  That normal should be used
  /// for displacement.
  ///
  /// Can return `None` for cases like disconnected vertices or degenerate triangles.
  fn separate_and_compute_normals_for_vertex(
    &mut self,
    smooth_fans_acc: &mut Vec<SmoothFan>,
    vtx_key: VertexKey,
  ) -> Option<Vec3> {
    let mut visited_edges = bitarr![0; 1024*8];
    let mut visited_faces = SmallVec::<[_; 16]>::new();
    let mut smooth_fan_faces: SmallVec<[FaceKey; 16]> = SmallVec::new();

    let mut vtx_normal_acc = NormalAcc::new();

    let edge_count = self.vertices[vtx_key].edges.len();
    // keeps track of which edges have been visited.  Indices match the indices of
    // `vtx.edges`
    if edge_count > 1024 * 8 {
      panic!("Vertex has too many edges; vtx_key={vtx_key:?}; edge_count={edge_count}",);
    }

    if edge_count < 3 {
      let mut seen_face_keys = SmallVec::<[_; 8]>::new();
      for &edge_key in &self.vertices[vtx_key].edges {
        for &face_key in &self.edges[edge_key].faces {
          if seen_face_keys.contains(&face_key) {
            continue;
          }
          seen_face_keys.push(face_key);

          vtx_normal_acc.add_face(vtx_key, face_key, &self.vertices, &self.faces);
        }
      }
      let computed_normal = vtx_normal_acc.get();
      let vtx = &mut self.vertices[vtx_key];
      vtx.shading_normal = computed_normal;
      return computed_normal;
    }

    loop {
      let visited_edges_slice = &mut visited_edges[..edge_count];
      // Track progress to prevent infinite loops on loose edges.
      let visited_count_start = visited_edges_slice.count_ones();

      let computed_fan_normal = self.walk_one_smooth_fan(
        vtx_key,
        visited_edges_slice,
        &mut visited_faces,
        &mut smooth_fan_faces,
        &mut vtx_normal_acc,
      );

      let all_faces_visited = visited_edges_slice.all();
      let made_progress = visited_edges_slice.count_ones() > visited_count_start;
      if all_faces_visited || !made_progress {
        // All faces from this fan can retain the same vertex, and we can assign
        // it the computed normal directly
        let vtx = &mut self.vertices[vtx_key];
        vtx.shading_normal = computed_fan_normal.or_else(|| vtx_normal_acc.get());

        break;
      }

      if !smooth_fan_faces.is_empty() {
        let Some(computed_fan_normal) = computed_fan_normal else {
          continue;
        };

        smooth_fans_acc.push(SmoothFan {
          old_key: vtx_key,
          face_keys: smooth_fan_faces.iter().copied().collect(),
          normal: computed_fan_normal,
        });

        smooth_fan_faces.clear();
      }
    }

    if cfg!(debug_assertions) && self.vertices[vtx_key].shading_normal.is_none() {
      panic!("Vertex {vtx_key:?} has no shading normal after walking smooth fans",);
    }

    vtx_normal_acc.get()
  }

  fn compute_displacement_normal(&mut self, vtx_key: VertexKey) -> Option<Vec3> {
    let mut visited_edges = bitarr![0; 1024*8];
    // TODO: should re-use these buffers and pass them in from outer
    let mut visited_faces = SmallVec::<[_; 16]>::new();
    let mut smooth_fan_faces: SmallVec<[FaceKey; 16]> = SmallVec::new();
    let mut vtx_normal_acc = NormalAcc::new();
    let edge_count = self.vertices[vtx_key].edges.len();

    loop {
      let visited_edges = &mut visited_edges[..edge_count];

      self.walk_one_smooth_fan(
        vtx_key,
        visited_edges,
        &mut visited_faces,
        &mut smooth_fan_faces,
        &mut vtx_normal_acc,
      );

      let all_faces_visited = visited_edges.all();
      if all_faces_visited {
        break;
      }

      smooth_fan_faces.clear();
    }

    let displacement_normal = vtx_normal_acc.get();
    let Some(displacement_normal) = displacement_normal else {
      // panic!(
      //   "Vertex {vtx_key:?} has no displacement normal after walking smooth fans; \
      //    visited_faces.len()={}",
      //   visited_faces.len()
      // );
      // return None;
      return Some(Vec3::zeros());
    };

    Some(displacement_normal)
  }

  /// Computes displacement normals for all vertices, replacing any existing
  /// values.
  pub fn compute_vertex_displacement_normals(&mut self) {
    let all_vtx_keys = self.vertices.keys().collect::<Vec<_>>();
    for vtx_key in all_vtx_keys {
      let displacement_normal = self.compute_displacement_normal(vtx_key);
      self.vertices[vtx_key].displacement_normal = displacement_normal;
    }
  }

  /// Does something akin to "shade auto-smooth" from Blender.  For each edge in the
  /// mesh, it is determined to be sharp or smooth based on the angle
  /// between it and the face that shares it.
  ///
  /// Then, each vertex is considered and potentially duplicated one or more
  /// times so that the normals of each are shared only with faces that are
  /// smooth wrt. each other.
  ///
  /// Heavily inspired by the Blender implementation:
  /// https://github.com/blender/blender/blob/a4aa5faa2008472413403600382f419280ac8b20/source/blender/bmesh/intern/bmesh_mesh_normals.cc#L1081
  pub fn separate_vertices_and_compute_normals(&mut self) {
    let all_vtx_keys = self.vertices.keys().collect::<Vec<_>>();
    // For each vertex in the mesh, we partition its edges into "smooth fans"
    // (as Blender calls them).  These are group of edges that all share the
    // vertex and are connected together by faces which each share one edge.
    //
    // They can either wrap all the way around the vertex or be bounded on
    // either side by a sharp edge.
    //
    // For each smooth fan, a duplicate vertex is created which gets a normal
    // computed by a weighted average of the face normals of all faces in the
    // fan.
    let mut smooth_fans = Vec::new();
    for vtx_key in all_vtx_keys {
      let computed_normal = self.separate_and_compute_normals_for_vertex(&mut smooth_fans, vtx_key);
      let vtx = &mut self.vertices[vtx_key];
      vtx.displacement_normal = computed_normal;
    }

    // We have to wait until we've walked all the fans for the mesh before splitting
    // vertices because if we do it dynamically while walking, the mesh topology
    // will get torn up and make smooth fan walking incorrect.
    for SmoothFan {
      old_key,
      face_keys,
      normal,
    } in smooth_fans
    {
      // We have to create a new vertex for the faces that are part of the
      // smooth fan so that it can have a distinct normal
      let (position, displacement_normal) = {
        let vtx = &self.vertices[old_key];
        (vtx.position, vtx.displacement_normal)
      };
      let new_vtx_key = self.vertices.insert(Vertex {
        position,
        shading_normal: Some(normal),
        displacement_normal,
        edges: SmallVec::new(),
        _padding: Default::default(),
      });

      for face_key in face_keys {
        self.replace_vertex_in_face(face_key, old_key, new_vtx_key);
      }
    }
  }

  pub fn to_raw_indexed(
    &self,
    include_shading_normals: bool,
    include_displacement_normals: bool,
    include_degenerate_faces: bool,
  ) -> OwnedIndexedMesh {
    let mut builder = OwnedIndexedMeshBuilder::with_capacity(
      self.vertices.len(),
      self.faces.len(),
      include_displacement_normals,
      include_shading_normals,
    );

    for face in self.faces.values() {
      if !include_degenerate_faces && face.is_degenerate(&self.vertices) {
        continue;
      }

      for &vtx_key in &face.vertices {
        let vtx = &self.vertices[vtx_key];
        builder.add_vtx(vtx_key, vtx)
      }
    }

    builder.build(self.transform)
  }

  /// Like `to_raw_indexed` but also returns a mapping from `VertexKey` to buffer index.
  pub fn to_raw_indexed_with_mapping(
    &self,
    include_shading_normals: bool,
    include_displacement_normals: bool,
    include_degenerate_faces: bool,
  ) -> (OwnedIndexedMesh, FxHashMap<VertexKey, usize>) {
    let mut builder = OwnedIndexedMeshBuilder::with_capacity(
      self.vertices.len(),
      self.faces.len(),
      include_displacement_normals,
      include_shading_normals,
    );

    for face in self.faces.values() {
      if !include_degenerate_faces && face.is_degenerate(&self.vertices) {
        continue;
      }

      for &vtx_key in &face.vertices {
        let vtx = &self.vertices[vtx_key];
        builder.add_vtx(vtx_key, vtx)
      }
    }

    builder.build_with_vtx_mapping(self.transform)
  }

  /// Works the same as `to_raw_indexed` but splits the triangles into multiple different meshes
  /// based on `partition_fn`.
  pub fn to_raw_indexed_multi<T: Hash + Eq>(
    &self,
    include_shading_normals: bool,
    include_displacement_normals: bool,
    partition_fn: impl Fn(&Face<FaceData>) -> T,
  ) -> Vec<OwnedIndexedMesh> {
    let mut out_meshes = FxHashMap::default();

    for face in self.faces.values() {
      if face.is_degenerate(&self.vertices) {
        continue;
      }

      let out_key = partition_fn(face);
      let builder = out_meshes.entry(out_key).or_insert_with(|| {
        OwnedIndexedMeshBuilder::new(include_displacement_normals, include_shading_normals)
      });

      for &vtx_key in &face.vertices {
        let vtx = &self.vertices[vtx_key];
        builder.add_vtx(vtx_key, vtx)
      }
    }

    out_meshes
      .into_values()
      .filter(|builder| !builder.is_empty())
      .map(|builder| builder.build(self.transform))
      .collect()
  }

  pub fn to_owned_mesh(&self, transform: Option<Mat4>) -> OwnedMesh {
    OwnedMesh {
      vertices: self
        .faces
        .values()
        .flat_map(|face| face.vertices.map(|vtx_key| self.vertices[vtx_key].position))
        .collect(),
      normals: Some(
        self
          .faces
          .values()
          .flat_map(|face| {
            face.vertices.map(|vtx_key| {
              self.vertices[vtx_key]
                .displacement_normal
                .unwrap_or_else(Vec3::zeros)
            })
          })
          .collect(),
      ),
      transform,
    }
  }

  /// Splits an edge in half, creating a new vertex in the middle.  All faces
  /// that include this edge are split into two faces with new edges being
  /// created for each.
  ///
  /// Returns the key of the new vertex.
  pub fn split_edge_cb(
    &mut self,
    edge_key_to_split: EdgeKey,
    split_pos: EdgeSplitPos,
    displacement_normal_method: DisplacementNormalMethod,
    mut face_split_cb: impl FnMut(&Self, FaceKey, FaceData, [FaceKey; 2]),
  ) -> VertexKey {
    let edge = self.edges.get(edge_key_to_split).unwrap_or_else(|| {
      panic!("Tried to split edge that doesn't exist; key={edge_key_to_split:?}")
    });
    let edge_id_to_split = edge.vertices;
    let [v0_key, v1_key] = edge_id_to_split;
    let [v0, v1] = [&self.vertices[v0_key], &self.vertices[v1_key]];

    let split_pos = split_pos.get(v0_key);
    let vm_position = v0.position.lerp(&v1.position, split_pos);

    let edge_displacement_normal = edge.displacement_normal;
    let faces_to_split = edge.faces.clone();

    let shading_normal = {
      let v0 = &self.vertices[edge_id_to_split[0]];
      let v1 = &self.vertices[edge_id_to_split[1]];
      v0.shading_normal
        .zip(v1.shading_normal)
        .map(|(n0, n1)| n0.lerp(&n1, split_pos).normalize())
    };

    let displacement_normal = match displacement_normal_method {
      DisplacementNormalMethod::Interpolate => {
        let v0 = &self.vertices[edge_id_to_split[0]];
        let v1 = &self.vertices[edge_id_to_split[1]];
        match v0.displacement_normal.zip(v1.displacement_normal) {
          Some((n0, n1)) => {
            let merged_normal = n0.lerp(&n1, split_pos).normalize();
            if merged_normal.x.is_nan() || merged_normal.y.is_nan() || merged_normal.z.is_nan() {
              panic!("Merged normal is NaN; n0={n0:?}; n1={n1:?}; merged_normal={merged_normal:?}");
              // Some(Vec3::new(random(), random(), random()).normalize())
              // None
            } else {
              Some(merged_normal)
            }
          }
          None => None,
        }
      }
      DisplacementNormalMethod::EdgeNormal => {
        // We set the displacement normal of the new vertex to be the same as the edge's
        // displacement normal.
        //
        // The other option here is average the displacement normals of the two vertices of the edge
        // being split.  However, this will cause the normals of other unrelated faces to be
        // averaged into this new vertex.  That leads to a sort of inflation effect where the
        // straight edges will become curved and "blown out".
        edge_displacement_normal
      }
    };

    let middle_vertex_key = self.vertices.insert(Vertex {
      position: vm_position,
      displacement_normal,
      shading_normal,
      edges: SmallVec::new(),
      _padding: Default::default(),
    });

    // Split each adjacent face
    for old_face_key in faces_to_split {
      let old_face = &self.faces[old_face_key];
      let old_face_normal = old_face.normal(&self.vertices);
      let [v0_key, v1_key, v2_key] = old_face.vertices;
      let split_edge_ix = if edge_id_to_split == sort_edge(v0_key, v1_key) {
        0
      } else if edge_id_to_split == sort_edge(v1_key, v2_key) {
        1
      } else {
        2
      };

      let vertex_keys = [v0_key, v1_key, v2_key, middle_vertex_key];
      let edge_displacement_normals = [
        self.edges[old_face.edges[0]].displacement_normal,
        self.edges[old_face.edges[1]].displacement_normal,
        self.edges[old_face.edges[2]].displacement_normal,
        Some(old_face_normal),
      ];
      let edge_sharpnesses = [
        self.edges[old_face.edges[0]].sharp,
        self.edges[old_face.edges[1]].sharp,
        self.edges[old_face.edges[2]].sharp,
        false,
      ];

      let orders = match split_edge_ix {
        0 => [[0, 3, 2], [3, 1, 2]],
        1 => [[0, 1, 3], [0, 3, 2]],
        2 => [[0, 1, 3], [1, 2, 3]],
        _ => unreachable!(),
      };

      let old_face_data = self.remove_face(old_face_key);

      let mut add_face = |order_ix: usize| {
        let order = &orders[order_ix];
        let face_key = self.add_face::<false>(
          [
            vertex_keys[order[0]],
            vertex_keys[order[1]],
            vertex_keys[order[2]],
          ],
          Default::default(),
        );
        let edge_keys = &self.faces[face_key].edges;
        for edge_ix in 0..3 {
          let edge_key = edge_keys[edge_ix];
          let edge = &mut self.edges[edge_key];
          edge.displacement_normal = edge_displacement_normals[order[edge_ix]];
          edge.sharp = edge_sharpnesses[order[edge_ix]];
        }
        face_key
      };

      let new_face_keys = [add_face(0), add_face(1)];
      face_split_cb(&*self, old_face_key, old_face_data, new_face_keys);
    }

    assert!(self.edges.remove(edge_key_to_split).is_none());

    middle_vertex_key
  }

  /// Splits an edge in half, creating a new vertex in the middle.  All faces
  /// that include this edge are split into two faces with new edges being
  /// created for each.
  ///
  /// Returns the key of the new vertex.
  pub fn split_edge(
    &mut self,
    edge_key_to_split: EdgeKey,
    split_pos: EdgeSplitPos,
    displacement_normal_method: DisplacementNormalMethod,
  ) -> VertexKey {
    self.split_edge_cb(
      edge_key_to_split,
      split_pos,
      displacement_normal_method,
      |_, _, _, _| {},
    )
  }

  pub fn get_edge_key(&self, mut vertices: [VertexKey; 2]) -> Option<EdgeKey> {
    sort_edge_inplace::<false>(&mut vertices);
    self.get_edge_key_from_sorted::<false>(vertices)
  }

  pub fn get_edge_key_from_sorted<const DENSE: bool>(
    &self,
    sorted_vtx_keys: [VertexKey; 2],
  ) -> Option<EdgeKey> {
    // iter through the vtx with fewer edges
    let vtx0 = unsafe { self.vertices.get_unchecked(sorted_vtx_keys[0]) };
    let vtx1 = unsafe { self.vertices.get_unchecked(sorted_vtx_keys[1]) };
    let vtx = if vtx0.edges.len() < vtx1.edges.len() {
      vtx0
    } else {
      vtx1
    };

    vtx
      .edges
      .iter()
      .find(|&&edge_key| {
        let edge = unsafe { self.edges.get_unchecked(edge_key) };
        if DENSE {
          vkey_ix(&edge.vertices[0]) == vkey_ix(&sorted_vtx_keys[0])
            && vkey_ix(&edge.vertices[1]) == vkey_ix(&sorted_vtx_keys[1])
        } else {
          edge.vertices == sorted_vtx_keys
        }
      })
      .copied()
  }

  /// Returns the key of the edge between the two provided vertices if it exists and creates it if
  /// not.
  ///
  /// If a new edge was created, the vertices of the edge will be updated to reference it.
  ///
  /// If `DENSE` is true, assumes that the `edges` slotmap is dense, meaning that it has never had
  /// elements removed from it, and uses a fastpath method for inserting new edges.
  pub fn get_or_create_edge<const DENSE: bool>(&mut self, mut vertices: [VertexKey; 2]) -> EdgeKey {
    sort_edge_inplace::<DENSE>(&mut vertices);

    match self.get_edge_key_from_sorted::<DENSE>(vertices) {
      Some(edge_key) => edge_key,
      None => {
        let new_edge = Edge {
          vertices,
          faces: SmallVec::new(),
          sharp: false,
          displacement_normal: None,
        };
        let edge_key = if DENSE {
          slotmap_insert_dense(&mut self.edges, new_edge)
        } else {
          self.edges.insert(new_edge)
        };

        for vert_key in vertices {
          let vert = unsafe { self.vertices.get_unchecked_mut(vert_key) };
          vert.edges.push(edge_key);
        }

        edge_key
      }
    }
  }

  pub fn add_face<const DENSE: bool>(
    &mut self,
    vertices: [VertexKey; 3],
    data: FaceData,
  ) -> FaceKey {
    let edges = [(0, 1), (1, 2), (2, 0)];
    let edge_keys =
      edges.map(|(a, b)| self.get_or_create_edge::<DENSE>([vertices[a], vertices[b]]));

    let cb = |face_key| {
      for edge_key in edge_keys {
        let edge = unsafe { self.edges.get_unchecked_mut(edge_key) };
        edge.faces.push(face_key);
      }

      Face {
        vertices,
        edges: edge_keys,
        data,
      }
    };

    if DENSE {
      slotmap_insert_dense_with_key(&mut self.faces, cb)
    } else {
      self.faces.insert_with_key(cb)
    }
  }

  pub fn remove_face(&mut self, face_key: FaceKey) -> FaceData {
    let face = self
      .faces
      .remove(face_key)
      .unwrap_or_else(|| panic!("Tried to remove face that doesn't exist; key={face_key:?}"));

    for edge_key in face.edges {
      let edge = self.edges.get_mut(edge_key).unwrap_or_else(|| {
        panic!(
          "Tried to get edge that doesn't exist; key={edge_key:?}. Was referenced by removed face \
           with key={face_key:?}",
        )
      });
      swap_retain_sv(&mut edge.faces, |&mut f| f != face_key);
      if edge.faces.is_empty() {
        let edge = self.edges.remove(edge_key).unwrap();
        for vert_key in edge.vertices {
          let vert = &mut self.vertices[vert_key];
          swap_retain_sv(&mut vert.edges, |&mut e| e != edge_key);
        }
      }
    }

    face.data
  }

  pub fn connected_components(&self) -> Vec<Vec<FaceKey>> {
    let mut visited =
      FxHashSet::with_capacity_and_hasher(self.faces.len(), FxBuildHasher::default());
    let mut components = Vec::new();

    for face_key in self.faces.keys() {
      if visited.contains(&face_key) {
        continue;
      }

      let mut component = Vec::new();
      let mut stack = vec![face_key];

      while let Some(cur_face_key) = stack.pop() {
        if visited.insert(cur_face_key) {
          component.push(cur_face_key);
          let face = &self.faces[cur_face_key];
          for &edge_key in &face.edges {
            for &adj_face_key in &self.edges[edge_key].faces {
              if !visited.contains(&adj_face_key) {
                stack.push(adj_face_key);
              }
            }
          }
        }
      }

      components.push(component);
    }

    components
  }

  /// For each connected component in the mesh, checks if the component is closed.  If it is, it
  /// determines if the component is oriented outward (if the face normals are facing the right
  /// way).  If they are not, the winding order and vertex normals for the component are flipped in
  /// place.
  pub fn orient_connected_components_outward(&mut self) {
    let components = self.connected_components();
    for component in components {
      let is_closed = component
        .iter()
        .flat_map(|&face_key| {
          let face = &self.faces[face_key];
          face.edges.iter().map(|&edge_key| &self.edges[edge_key])
        })
        .all(|edge| edge.faces.len() == 2);

      if !is_closed {
        continue;
      }

      let uniq_vtx_keys: FxHashSet<_> = component
        .iter()
        .flat_map(|&face_key| {
          let face = &self.faces[face_key];
          face.vertices.iter().copied()
        })
        .collect();

      let signed_volume_of_component = {
        let mut v = 0.0f32;
        for &face_key in &component {
          let face = &self.faces[face_key];
          let [i0, i1, i2] = face.vertices;
          let a = self.vertices[i0].position;
          let b = self.vertices[i1].position;
          let c = self.vertices[i2].position;
          v += a.cross(&b).dot(&c);
        }
        v / 6.0
      };
      log::info!("signed_volume_of_component: {signed_volume_of_component}");
      let is_outward_facing = signed_volume_of_component > 0.;

      if is_outward_facing {
        continue;
      }

      // Flip the winding order and vertex normals of all faces in this component
      for &face_key in &component {
        let face = &mut self.faces[face_key];
        face.vertices.swap(0, 2);
      }

      for vtx_key in uniq_vtx_keys {
        let vtx = &mut self.vertices[vtx_key];
        vtx.shading_normal = vtx.shading_normal.map(|n| -n);
        vtx.displacement_normal = vtx.displacement_normal.map(|n| -n);
      }
    }
  }

  /// Removes all faces from the mesh that contain an edge is not shared by exactly two faces.
  pub fn remove_nonmanifold_faces(&mut self) -> usize {
    let mut total_removed_count = 0;

    loop {
      let nonmanifold_face_keys: Vec<FaceKey> = self
        .faces
        .iter()
        .filter_map(|(face_key, face)| {
          let has_nonmanifold_edge = face.edges.iter().any(|&edge_key| {
            let edge = &self.edges[edge_key];
            edge.faces.len() != 2
          });
          if has_nonmanifold_edge {
            Some(face_key)
          } else {
            None
          }
        })
        .collect();

      let removed_count = nonmanifold_face_keys.len();
      if removed_count == 0 {
        break;
      }
      total_removed_count += removed_count;

      for face_key in nonmanifold_face_keys {
        self.remove_face(face_key);
      }
    }

    total_removed_count
  }

  /// Removes all degenerate faces from the mesh.
  pub fn cleanup_degenerate_triangles_cb(&mut self, mut cb: impl FnMut(FaceKey, FaceData)) {
    let all_face_keys = self.faces.keys().collect::<Vec<_>>();
    for face_key in all_face_keys {
      let face = &self.faces[face_key];
      if face.is_degenerate(&self.vertices) {
        let face_data = self.remove_face(face_key);
        cb(face_key, face_data);
      }
    }
  }

  pub fn cleanup_degenerate_triangles(&mut self) {
    self.cleanup_degenerate_triangles_cb(|_, _| {});
  }

  pub fn is_empty(&self) -> bool {
    self.vertices.is_empty()
  }

  pub fn subdivide_by_plane(&mut self, plane: &Plane) {
    let face_keys: Vec<FaceKey> = self.faces.keys().collect();
    let mut created_vertices = FxHashMap::<[VertexKey; 2], VertexKey>::default();

    for face_key in face_keys {
      if !self.faces.contains_key(face_key) {
        continue;
      }

      let face = &self.faces[face_key];
      let mut polygon_type = PolygonClass::Coplanar;
      let mut types = [PolygonClass::Coplanar; 3];
      let mut dists = [0.; 3];

      for (i, &v_key) in face.vertices.iter().enumerate() {
        let dist = plane.normal.dot(&self.vertices[v_key].position) - plane.w;
        dists[i] = dist;
        types[i] = if dist < -EPSILON {
          PolygonClass::Back
        } else if dist > EPSILON {
          PolygonClass::Front
        } else {
          PolygonClass::Coplanar
        };
        polygon_type = polygon_type | types[i];
      }

      if polygon_type == PolygonClass::Spanning {
        let mut f: ArrayVec<VertexKey, 4> = ArrayVec::new();
        let mut b: ArrayVec<VertexKey, 4> = ArrayVec::new();
        let face_verts = self.faces[face_key].vertices;

        for i in 0..3 {
          let j = (i + 1) % 3;
          let ti = types[i];
          let tj = types[j];
          let vi_key = face_verts[i];
          let vj_key = face_verts[j];

          if ti != PolygonClass::Back {
            f.push(vi_key);
          }
          if ti != PolygonClass::Front {
            b.push(vi_key);
          }

          if (ti | tj) == PolygonClass::Spanning {
            let edge_key = sort_edge(vi_key, vj_key);
            let new_v_key = if let Some(&key) = created_vertices.get(&edge_key) {
              key
            } else {
              let t = dists[i] / (dists[i] - dists[j]);
              let v_i = &self.vertices[vi_key];
              let v_j = &self.vertices[vj_key];
              let new_v_pos = v_i.position.lerp(&v_j.position, t);

              let shading_normal = match (v_i.shading_normal, v_j.shading_normal) {
                (Some(n0), Some(n1)) => Some(n0.lerp(&n1, t).normalize()),
                _ => None,
              };
              let displacement_normal = match (v_i.displacement_normal, v_j.displacement_normal) {
                (Some(n0), Some(n1)) => Some(n0.lerp(&n1, t).normalize()),
                _ => None,
              };

              let key = self.vertices.insert(Vertex {
                position: new_v_pos,
                shading_normal,
                displacement_normal,
                edges: SmallVec::new(),
                _padding: Default::default(),
              });
              created_vertices.insert(edge_key, key);
              key
            };

            f.push(new_v_key);
            b.push(new_v_key);
          }
        }

        self.remove_face(face_key);

        if f.len() >= 3 {
          self.add_face::<false>([f[0], f[1], f[2]], Default::default());
          if f.len() == 4 {
            self.add_face::<false>([f[0], f[2], f[3]], Default::default());
          }
        }

        if b.len() >= 3 {
          self.add_face::<false>([b[0], b[1], b[2]], Default::default());
          if b.len() == 4 {
            self.add_face::<false>([b[0], b[2], b[3]], Default::default());
          }
        }
      }
    }
  }

  pub fn compute_aabb(&self, transform: &Matrix4<f32>) -> Aabb {
    if self.vertices.is_empty() {
      return Aabb::new_invalid();
    }

    let mut mins = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut maxs = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

    for vtx in self.vertices.values() {
      let transformed_pos = (transform * vtx.position.push(1.)).xyz();
      mins = Vec3::new(
        mins.x.min(transformed_pos.x),
        mins.y.min(transformed_pos.y),
        mins.z.min(transformed_pos.z),
      );
      maxs = Vec3::new(
        maxs.x.max(transformed_pos.x),
        maxs.y.max(transformed_pos.y),
        maxs.z.max(transformed_pos.z),
      );
    }

    Aabb::new(
      Point::new(mins.x, mins.y, mins.z),
      Point::new(maxs.x, maxs.y, maxs.z),
    )
  }

  pub fn build_trimesh(&self, transform: &Matrix4<f32>) -> Result<TriMesh, TriMeshBuilderError> {
    let OwnedIndexedMesh {
      mut vertices,
      mut indices,
      ..
    } = self.to_raw_indexed(false, false, true);

    vertices.shrink_to_fit();
    assert_eq!(vertices.len() % 3, 0,);
    assert_eq!(vertices.capacity() % 3, 0);

    {
      let vec3_view = unsafe {
        std::slice::from_raw_parts_mut(vertices.as_ptr() as *mut Vec3, vertices.len() / 3)
      };
      for v in vec3_view.iter_mut() {
        *v = (transform * v.push(1.)).xyz();
      }
    }

    let point_vertices = unsafe {
      Vec::from_raw_parts(
        vertices.as_ptr() as *mut Point<f32>,
        vertices.len() / 3,
        vertices.capacity() / 3,
      )
    };
    std::mem::forget(vertices);

    let arr_indices = if std::mem::size_of::<usize>() == std::mem::size_of::<u32>() {
      assert_eq!(indices.len() % 3, 0);
      indices.shrink_to_fit();
      assert_eq!(indices.capacity() % 3, 0);
      let arr_indices = unsafe {
        Vec::from_raw_parts(
          indices.as_ptr() as *mut [u32; 3],
          indices.len() / 3,
          indices.capacity() / 3,
        )
      };
      std::mem::forget(indices);
      arr_indices
    } else {
      indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|[a, b, c]| [*a as u32, *b as u32, *c as u32])
        .collect()
    };

    TriMesh::new(point_vertices, arr_indices)
  }

  pub fn new_box(width: f32, height: f32, depth: f32) -> Self {
    let half_width = width / 2.;
    let half_height = height / 2.;
    let half_depth = depth / 2.;

    LinkedMesh::from_indexed_vertices(
      &[
        Vec3::new(-half_width, -half_height, -half_depth),
        Vec3::new(half_width, -half_height, -half_depth),
        Vec3::new(half_width, half_height, -half_depth),
        Vec3::new(-half_width, half_height, -half_depth),
        Vec3::new(-half_width, -half_height, half_depth),
        Vec3::new(half_width, -half_height, half_depth),
        Vec3::new(half_width, half_height, half_depth),
        Vec3::new(-half_width, half_height, half_depth),
      ],
      &[
        0, 2, 1, 0, 3, 2, //
        4, 5, 6, 4, 6, 7, //
        0, 1, 5, 0, 5, 4, //
        2, 3, 7, 2, 7, 6, //
        0, 7, 3, 0, 4, 7, //
        1, 2, 6, 1, 6, 5, //
      ],
      None,
      None,
    )
  }

  /// Generates an icosphere mesh with the given `radius` and number of recursive `subdivisions`.
  pub fn new_icosphere(radius: f32, subdivisions: u32) -> Self {
    // let t = (1. + 5.0_f32.sqrt()) / 2.;
    // let mut vertices = vec![
    //   Vec3::new(-1., t, 0.).normalize(),
    //   Vec3::new(1., t, 0.).normalize(),
    //   Vec3::new(-1., -t, 0.).normalize(),
    //   Vec3::new(1., -t, 0.).normalize(),
    //   Vec3::new(0., -1., t).normalize(),
    //   Vec3::new(0., 1., t).normalize(),
    //   Vec3::new(0., -1., -t).normalize(),
    //   Vec3::new(0., 1., -t).normalize(),
    //   Vec3::new(t, 0., -1.).normalize(),
    //   Vec3::new(t, 0., 1.).normalize(),
    //   Vec3::new(-t, 0., -1.).normalize(),
    //   Vec3::new(-t, 0., 1.).normalize(),
    // ];

    // same as above but pre-computed::
    const BASE_VERTICES: &[Vec3] = &[
      Vec3::new(-0.5257311, 0.8506508, 0.),
      Vec3::new(0.5257311, 0.8506508, 0.),
      Vec3::new(-0.5257311, -0.8506508, 0.),
      Vec3::new(0.5257311, -0.8506508, 0.),
      Vec3::new(0., -0.5257311, 0.8506508),
      Vec3::new(0., 0.5257311, 0.8506508),
      Vec3::new(0., -0.5257311, -0.8506508),
      Vec3::new(0., 0.5257311, -0.8506508),
      Vec3::new(0.8506508, 0., -0.5257311),
      Vec3::new(0.8506508, 0., 0.5257311),
      Vec3::new(-0.8506508, 0., -0.5257311),
      Vec3::new(-0.8506508, 0., 0.5257311),
    ];

    const BASE_FACES: &[[usize; 3]] = &[
      [0, 11, 5],
      [0, 5, 1],
      [0, 1, 7],
      [0, 7, 10],
      [0, 10, 11],
      [1, 5, 9],
      [5, 11, 4],
      [11, 10, 2],
      [10, 7, 6],
      [7, 1, 8],
      [3, 9, 4],
      [3, 4, 2],
      [3, 2, 6],
      [3, 6, 8],
      [3, 8, 9],
      [4, 9, 5],
      [2, 4, 11],
      [6, 2, 10],
      [8, 6, 7],
      [9, 8, 1],
    ];

    let total_vtx_count = 10 * (4usize).pow(subdivisions) + 2;

    let mut faces: Vec<[usize; 3]> = Vec::new();
    let mut vertices: Vec<Vec3> = Vec::with_capacity(total_vtx_count);
    vertices.extend_from_slice(BASE_VERTICES);

    if subdivisions == 0 {
      faces = BASE_FACES.to_vec();
    }

    for _ in 0..subdivisions {
      let old_faces: &[_] = if faces.is_empty() { BASE_FACES } else { &faces };
      let new_face_count = old_faces.len() * 4;
      let mut new_faces = Vec::with_capacity(new_face_count);
      let mut midpoint_cache = FxHashMap::<(usize, usize), usize>::default();

      let mut get_midpoint = |i1: usize, i2: usize| {
        let key = if i1 < i2 { (i1, i2) } else { (i2, i1) };
        if let Some(&idx) = midpoint_cache.get(&key) {
          return idx;
        }
        let v1 = *unsafe { vertices.get_unchecked(i1) };
        let v2 = *unsafe { vertices.get_unchecked(i2) };
        let midpoint = ((v1 + v2) * 0.5).normalize();
        let vtx_ix = vertices.len();
        vertices.push(midpoint);
        midpoint_cache.insert(key, vtx_ix);
        vtx_ix
      };

      for &[i0, i1, i2] in old_faces {
        let a = get_midpoint(i0, i1);
        let b = get_midpoint(i1, i2);
        let c = get_midpoint(i2, i0);

        new_faces.push([i0, a, c]);
        new_faces.push([i1, b, a]);
        new_faces.push([i2, c, b]);
        new_faces.push([a, b, c]);
      }

      assert_eq!(new_faces.len(), new_face_count);
      faces = new_faces;
    }

    assert_eq!(vertices.len(), total_vtx_count);
    for v in &mut vertices {
      *v *= radius;
    }

    if std::mem::size_of::<usize>() == std::mem::size_of::<u32>() {
      let indices: &[u32] =
        unsafe { std::slice::from_raw_parts(faces.as_ptr() as *const u32, faces.len() * 3) };
      return LinkedMesh::from_indexed_vertices(&vertices, indices, None, None);
    }

    let indices: Vec<u32> = faces
      .into_iter()
      .flat_map(|[i0, i1, i2]| [i0 as u32, i1 as u32, i2 as u32])
      .collect();

    LinkedMesh::from_indexed_vertices(&vertices, &indices, None, None)
  }

  pub fn new_cylinder(
    radius: f32,
    height: f32,
    radial_segments: usize,
    height_segments: usize,
  ) -> Self {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    generate_radial_vertices(
      &mut vertices,
      radial_segments,
      height_segments,
      |_h| radius,
      |h| -height / 2. + h * height,
    );

    generate_radial_side_indices(&mut indices, radial_segments, height_segments);

    let bottom_center_ix = vertices.len() as u32;
    vertices.push(Vec3::new(0., -height / 2., 0.));
    let top_center_ix = vertices.len() as u32;
    vertices.push(Vec3::new(0., height / 2., 0.));

    for (center_ix, reverse_winding, ring_offset) in [
      (bottom_center_ix, false, 0usize),
      (top_center_ix, true, height_segments * radial_segments),
    ] {
      generate_cap_indices(
        &mut indices,
        center_ix,
        radial_segments,
        ring_offset,
        reverse_winding,
      );
    }

    LinkedMesh::from_indexed_vertices(&vertices, &indices, None, None)
  }

  /// Generates a cone mesh.  The base of the cone is centered at the origin, and the cone extends
  /// upwards along the positive Y axis.  The apex (point) of the cone is at the top.
  pub fn new_cone(
    radius: f32,
    height: f32,
    radial_segments: usize,
    height_segments: usize,
  ) -> Self {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();

    for h in 0..height_segments {
      let h_norm = h as f32 / height_segments as f32;
      let y = h_norm * height;
      let r = radius * (1.0 - h_norm);
      for seg in 0..radial_segments {
        let theta = (seg as f32 / radial_segments as f32) * 2. * PI;
        let x = r * theta.cos();
        let z = r * theta.sin();
        vertices.push(Vec3::new(x, y, z));
      }
    }

    for h in 0..(height_segments - 1) {
      for r in 0..radial_segments {
        let v0 = h * radial_segments + r;
        let v1 = v0 + radial_segments;
        let next_r = (r + 1) % radial_segments;
        let v0_next = h * radial_segments + next_r;
        let v1_next = v0_next + radial_segments;

        indices.push(v0 as u32);
        indices.push(v1 as u32);
        indices.push(v0_next as u32);
        indices.push(v1 as u32);
        indices.push(v1_next as u32);
        indices.push(v0_next as u32);
      }
    }

    let apex_ix = vertices.len() as u32;
    vertices.push(Vec3::new(0., height, 0.));

    let top_ring_offset = (height_segments - 1) * radial_segments;
    generate_cap_indices(
      &mut indices,
      apex_ix,
      radial_segments,
      top_ring_offset,
      true,
    );

    let bottom_center_ix = vertices.len() as u32;
    vertices.push(Vec3::new(0., 0., 0.));
    generate_cap_indices(&mut indices, bottom_center_ix, radial_segments, 0, false);

    LinkedMesh::from_indexed_vertices(&vertices, &indices, None, None)
  }

  /// Generates a flat grid mesh in the XZ plane centered at the origin with the specified size and
  /// number of subdivisions.
  pub fn new_grid(
    size_x: f32,
    size_z: f32,
    subdivisions_x: usize,
    subdivisions_z: usize,
    flipped: bool,
  ) -> Self {
    let half_size_x = size_x / 2.;
    let half_size_z = size_z / 2.;

    let step_x = size_x / subdivisions_x as f32;
    let step_z = size_z / subdivisions_z as f32;

    let mut vertices = Vec::with_capacity((subdivisions_x + 1) * (subdivisions_z + 1));
    for z in 0..=subdivisions_z {
      for x in 0..=subdivisions_x {
        vertices.push(Vec3::new(
          -half_size_x + x as f32 * step_x,
          0.,
          -half_size_z + z as f32 * step_z,
        ));
      }
    }

    let mut indices = Vec::with_capacity(subdivisions_x * subdivisions_z * 6);
    for z in 0..subdivisions_z {
      for x in 0..subdivisions_x {
        let v0 = z * (subdivisions_x + 1) + x;
        let v1 = v0 + 1;
        let v2 = v0 + (subdivisions_x + 1);
        let v3 = v2 + 1;

        if flipped {
          indices.push(v0 as u32);
          indices.push(v1 as u32);
          indices.push(v2 as u32);

          indices.push(v1 as u32);
          indices.push(v3 as u32);
          indices.push(v2 as u32);
        } else {
          indices.push(v0 as u32);
          indices.push(v2 as u32);
          indices.push(v1 as u32);

          indices.push(v1 as u32);
          indices.push(v2 as u32);
          indices.push(v3 as u32);
        }
      }
    }

    LinkedMesh::from_indexed_vertices(&vertices, &indices, None, None)
  }

  pub fn new_utah_teapot() -> Self {
    let verts = unsafe {
      std::slice::from_raw_parts(
        UTAH_TEAPOT_VERTICES.as_ptr() as *const Vec3,
        UTAH_TEAPOT_VERTICES.len(),
      )
    };
    let indices = UTAH_TEAPOT_INDICES;
    let indices = indices.iter().map(|&i| i as u32).collect::<Vec<_>>();

    Self::from_indexed_vertices(verts, &indices, None, None)
  }

  pub fn new_stanford_bunny() -> Self {
    let verts = unsafe {
      std::slice::from_raw_parts(
        STANFORD_BUNNY_VERTICES.as_ptr() as *const Vec3,
        STANFORD_BUNNY_VERTICES.len(),
      )
    };
    let indices = STANFORD_BUNNY_INDICES;
    let indices = indices.iter().map(|&i| i as u32).collect::<Vec<_>>();

    Self::from_indexed_vertices(verts, &indices, None, None)
  }

  // pub fn new_suzanne() -> Self {
  //   let verts = unsafe {
  //     std::slice::from_raw_parts(
  //       SUZANNE_VERTICES.as_ptr() as *const Vec3,
  //       SUZANNE_VERTICES.len(),
  //     )
  //   };
  //   let indices = SUZANNE_INDICES;
  //   let indices = indices.iter().map(|&i| i as u32).collect::<Vec<_>>();

  //   Self::from_indexed_vertices(verts, &indices, None, None)
  // }

  /// Actually flips the winding order of all faces in the mesh and updates all shading +
  /// displacement normals accordingly.
  pub fn flip_normals(&mut self) {
    for face in self.faces.values_mut() {
      face.vertices.swap(0, 2);
    }

    for vtx in self.vertices.values_mut() {
      vtx.shading_normal = vtx.shading_normal.map(|n| -n);
      vtx.displacement_normal = vtx.displacement_normal.map(|n| -n);
    }
  }
}

impl<T: Default> From<TriMesh> for LinkedMesh<T> {
  fn from(trimesh: TriMesh) -> Self {
    // TODO: optimize using the fastpath slotmap methods
    let mut mesh: LinkedMesh<T> =
      LinkedMesh::new(trimesh.vertices().len(), trimesh.indices().len(), None);
    let mut vtx_keys = Vec::with_capacity(trimesh.vertices().len());
    for vtx in trimesh.vertices() {
      let vtx_key = mesh.vertices.insert(Vertex {
        position: Vec3::new(vtx.x, vtx.y, vtx.z),
        shading_normal: None,
        displacement_normal: None,
        edges: SmallVec::new(),
        _padding: Default::default(),
      });
      vtx_keys.push(vtx_key);
    }

    for &[v0, v1, v2] in trimesh.indices() {
      mesh.add_face::<true>(
        [
          vtx_keys[v0 as usize],
          vtx_keys[v1 as usize],
          vtx_keys[v2 as usize],
        ],
        T::default(),
      );
    }

    mesh
  }
}

#[cfg(test)]
mod tests {
  use crate::slotmap_utils::fkey;

  use super::*;

  #[test]
  fn basic_edge_split() {
    let verts = [
      Vec3::new(-1., 0., 0.),
      Vec3::new(0., 1., 0.),
      Vec3::new(1., 0., 0.),
      Vec3::new(0., -1., 0.),
    ];

    // 1 -> 0 -> 3
    // 2 -> 1 -> 3
    let indices = [1, 0, 3, 2, 1, 3];

    let mut mesh: LinkedMesh<()> = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
    let middle_edge_key = mesh
      .iter_edges()
      .find(|(_, e)| e.faces.len() == 2)
      .unwrap()
      .0;
    mesh.split_edge(
      middle_edge_key,
      EdgeSplitPos::middle(),
      DisplacementNormalMethod::EdgeNormal,
    );

    assert_eq!(mesh.iter_vertices().count(), 5);
    let center_vtx_key = mesh
      .iter_vertices()
      .find(|(_, v)| v.position == Vec3::zeros())
      .unwrap()
      .0;
    let center_vtx = &mesh.vertices[center_vtx_key];
    assert_eq!(center_vtx.edges.len(), 4);

    for &edge_key in &center_vtx.edges {
      if edge_key == middle_edge_key {
        continue;
      }

      let edge = &mesh.edges[edge_key];
      assert!(edge.faces.len() <= 2);

      for vtx_key in edge.vertices {
        if vtx_key == center_vtx_key {
          continue;
        }

        let vtx = &mesh.vertices[vtx_key];
        assert!(vtx.edges.len() <= 3);
      }
    }

    /// Returns true if the two triangles have the same vertices, regardless of
    /// order (as long as the winding order is consistent)
    fn tris_equal(t0: [[i32; 3]; 3], t1: [[i32; 3]; 3]) -> bool {
      return t0 == t1 || t0 == [t1[1], t1[2], t1[0]] || t0 == [t1[2], t1[0], t1[1]];
    }

    let expected_unindexed_verts = [
      [[0, 0, 0], [0, 1, 0], [-1, 0, 0]],
      [[1, 0, 0], [0, 1, 0], [0, 0, 0]],
      [[1, 0, 0], [0, 0, 0], [0, -1, 0]],
      [[0, -1, 0], [0, 0, 0], [-1, 0, 0]],
    ];
    let actual_indexed_mesh = mesh.to_raw_indexed(true, true, false);
    assert_eq!(actual_indexed_mesh.indices.len(), 12);
    let actual_verts = actual_indexed_mesh
      .indices
      .chunks(3)
      .map(|v| {
        let v0 = &actual_indexed_mesh.vertices[(v[0] * 3)..(v[0] * 3 + 3)];
        let v1 = &actual_indexed_mesh.vertices[(v[1] * 3)..(v[1] * 3 + 3)];
        let v2 = &actual_indexed_mesh.vertices[(v[2] * 3)..(v[2] * 3 + 3)];
        [
          [v0[0] as i32, v0[1] as i32, v0[2] as i32],
          [v1[0] as i32, v1[1] as i32, v1[2] as i32],
          [v2[0] as i32, v2[1] as i32, v2[2] as i32],
        ]
      })
      .collect::<Vec<_>>();

    assert_eq!(actual_verts.len(), expected_unindexed_verts.len());
    for expected_tri in &expected_unindexed_verts {
      assert!(
        actual_verts
          .iter()
          .any(|actual_tri| tris_equal(*actual_tri, *expected_tri)),
        "Expected triangle {:?} not found in actual triangles: {:?}",
        expected_tri,
        actual_verts
      );
    }
  }

  #[test]
  fn basic_smooth_fan() {
    // Smooth fan that consists of 3 edges and 2 faces, smooth all the way around
    let verts = [
      // vertex that we'll fan around
      Vec3::new(0., 0., 0.),
      Vec3::new(-1., 1., 0.),
      Vec3::new(0., 1., 0.),
      Vec3::new(1., 1., 0.),
    ];
    let indices = [0, 2, 1, 0, 3, 2];

    let mut mesh: LinkedMesh<()> = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
    let vtx_key = vkey(1, 1);
    let mut visited_edges = bitarr![0; 1024];
    let visited_edges = &mut visited_edges[..mesh.vertices[vtx_key].edges.len()];
    assert_eq!(visited_edges.len(), 3);
    let mut smooth_fan_faces = SmallVec::<[_; 16]>::new();
    let mut visited_faces = SmallVec::<[_; 16]>::new();

    mesh.walk_one_smooth_fan(
      vtx_key,
      visited_edges,
      &mut visited_faces,
      &mut smooth_fan_faces,
      &mut NormalAcc::new(),
    );

    assert!(visited_edges.all());
    let smooth_fan_faces: FxHashSet<_> = smooth_fan_faces.iter().copied().collect();
    assert_eq!(smooth_fan_faces.len(), 2);
  }

  fn test_walk_smooth_fan(
    mesh: &mut LinkedMesh,
    center_vtx_key: VertexKey,
  ) -> Vec<FxHashSet<FaceKey>> {
    let center_vtx_edge_count = mesh.vertices[center_vtx_key].edges.len();
    let mut visited_edges = bitarr![0; 1024];
    let visited_edges = &mut visited_edges[..center_vtx_edge_count];
    let mut smooth_fan_faces = SmallVec::<[_; 16]>::new();
    let mut visited_faces = SmallVec::<[_; 16]>::new();

    let mut smooth_fans = Vec::new();
    loop {
      mesh.walk_one_smooth_fan(
        center_vtx_key,
        visited_edges,
        &mut visited_faces,
        &mut smooth_fan_faces,
        &mut NormalAcc::new(),
      );

      let uniq_smooth_fan_faces: FxHashSet<_> = smooth_fan_faces.iter().copied().collect();
      smooth_fans.push(uniq_smooth_fan_faces);
      smooth_fan_faces.clear();

      if visited_edges.all() {
        break;
      }
    }

    smooth_fans
  }

  #[test]
  fn two_triangles_joined_at_one_point() {
    let verts = [
      Vec3::new(0., 0., 0.),
      Vec3::new(-1., 1., 0.),
      Vec3::new(-1., 0., 0.),
      Vec3::new(1., 1., 0.),
      Vec3::new(1., 0., 0.),
    ];
    let indices = [0, 1, 2, 0, 4, 3];

    let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
    let vtx_key = vkey(1, 1);

    let smooth_fans = test_walk_smooth_fan(&mut mesh, vtx_key);

    assert_eq!(smooth_fans.len(), 2);
    let expected_smooth_fans = [[fkey(1, 1)], [fkey(2, 1)]]
      .into_iter()
      .map(|fan| fan.into_iter().collect::<FxHashSet<_>>())
      .collect::<Vec<_>>();
    for smooth_fan in &smooth_fans {
      assert!(expected_smooth_fans.contains(smooth_fan));
    }

    // walking from any other vertex should only have a single smooth fan with a
    // single face
    for vtx_key in [
      unsafe { std::mem::transmute((1u32, 2u32)) },
      unsafe { std::mem::transmute((1u32, 3u32)) },
      unsafe { std::mem::transmute((1u32, 4u32)) },
      unsafe { std::mem::transmute((1u32, 5u32)) },
    ] {
      let mut visited_edges = bitarr![0; 1024];
      let visited_edges = &mut visited_edges[..mesh.vertices[vtx_key].edges.len()];
      let mut smooth_fan_faces = SmallVec::<[_; 16]>::new();
      let mut visited_faces = SmallVec::<[_; 16]>::new();

      let mut smooth_fans = Vec::new();
      loop {
        mesh.walk_one_smooth_fan(
          vtx_key,
          visited_edges,
          &mut visited_faces,
          &mut smooth_fan_faces,
          &mut NormalAcc::new(),
        );

        let uniq_smooth_fan_faces: Vec<_> = smooth_fan_faces.iter().copied().collect();
        smooth_fans.push(uniq_smooth_fan_faces);
        smooth_fan_faces.clear();

        if visited_edges.all() {
          break;
        }
      }

      assert_eq!(smooth_fans.len(), 1);
      assert_eq!(smooth_fans[0].len(), 1);
    }
  }

  #[test]
  fn one_sharp_edge() {
    // Same as `basic_smooth_fan` but with a sharp edge between the two
    // triangles.  The result should be two separate smooth fans, each
    // containing one face.
    let verts = [
      // vertex that we'll fan around
      Vec3::new(0., 0., 0.),
      Vec3::new(-1., 1., 0.),
      Vec3::new(0., 1., 0.),
      Vec3::new(1., 1., 0.),
    ];
    let indices = [0, 2, 1, 0, 3, 2];

    let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
    // mark the edge between the two triangles as sharp
    let sharp_edge_key = mesh
      .edges
      .iter_mut()
      .find_map(|(edge_key, edge)| {
        if edge.faces.len() == 2 {
          Some(edge_key)
        } else {
          None
        }
      })
      .unwrap();
    mesh.edges[sharp_edge_key].sharp = true;

    let vtx_key = vkey(1, 1);
    let smooth_fans = test_walk_smooth_fan(&mut mesh, vtx_key);

    assert_eq!(smooth_fans.len(), 2);
  }

  fn build_full_fan_mesh_with_sharp_edges(sharp_edges_vtx_coords: &[[[i32; 2]; 2]]) -> LinkedMesh {
    let verts = [[0, 0], [-1, 0], [0, 1], [1, 0], [0, -1]]
      .into_iter()
      .map(|[x, y]| Vec3::new(x as f32, y as f32, 0.))
      .collect::<Vec<_>>();
    let indices = [0, 2, 1, 0, 3, 2, 0, 1, 4, 0, 4, 3];

    let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
    let sharp_edge_keys = mesh
      .edges
      .iter()
      .filter_map(|(edge_key, edge)| {
        let [v0_key, v1_key] = edge.vertices;
        let [v0_pos, v1_pos] = [
          [
            mesh.vertices[v0_key].position.x as i32,
            mesh.vertices[v0_key].position.y as i32,
          ],
          [
            mesh.vertices[v1_key].position.x as i32,
            mesh.vertices[v1_key].position.y as i32,
          ],
        ];

        for &[sharp_edge_v0_pos, sharp_edge_v1_pos] in sharp_edges_vtx_coords {
          if (sharp_edge_v0_pos == v0_pos && sharp_edge_v1_pos == v1_pos)
            || (sharp_edge_v0_pos == v1_pos && sharp_edge_v1_pos == v0_pos)
          {
            return Some(edge_key);
          }
        }

        None
      })
      .collect::<Vec<_>>();
    assert_eq!(sharp_edge_keys.len(), sharp_edges_vtx_coords.len());

    for sharp_edge_key in sharp_edge_keys {
      mesh.edges.get_mut(sharp_edge_key).unwrap().sharp = true;
    }

    mesh
  }

  #[test]
  fn full_fan_two_sharp_edges() {
    let mut mesh = build_full_fan_mesh_with_sharp_edges(&[[[0, 0], [0, 1]], [[0, 0], [0, -1]]]);

    let center_vtx_key = vkey(1, 1);
    let smooth_fans = test_walk_smooth_fan(&mut mesh, center_vtx_key);

    // faces 1 + 3 should be together, and 2 + 4 should be together
    assert_eq!(smooth_fans.len(), 2);
    let expected_smooth_fans = [[fkey(1, 1), fkey(3, 1)], [fkey(2, 1), fkey(4, 1)]]
      .into_iter()
      .map(|v| v.into_iter().collect::<FxHashSet<_>>())
      .collect::<Vec<_>>();

    for smooth_fan in &smooth_fans {
      assert!(expected_smooth_fans
        .iter()
        .any(|expected| expected == smooth_fan));
    }
  }

  /// A full fan with one sharp edges will actually result in only a single
  /// smooth fan because there's no way to split the vertex without breaking
  /// topology
  #[test]
  fn full_fan_one_sharp_edge() {
    let mut mesh = build_full_fan_mesh_with_sharp_edges(&[[[0, 0], [0, 1]]]);

    let center_vtx_key = vkey(1, 1);
    let smooth_fans = test_walk_smooth_fan(&mut mesh, center_vtx_key);

    assert_eq!(smooth_fans.len(), 1);
    let expected_smooth_fan = [fkey(1, 1), fkey(2, 1), fkey(3, 1), fkey(4, 1)]
      .into_iter()
      .collect::<FxHashSet<_>>();

    assert_eq!(smooth_fans[0], expected_smooth_fan);
  }

  #[test]
  fn cube_two_manifold() {
    let mesh: LinkedMesh<()> = LinkedMesh::new_box(1., 1., 1.);

    mesh
      .check_is_manifold::<true>()
      .expect("basic cube mesh should be two-manifold");
  }

  #[test]
  fn subdivide_by_plane_manifold() {
    let mut mesh: LinkedMesh<()> = LinkedMesh::new_box(1., 1., 1.);
    let plane = Plane {
      normal: Vec3::new(1., 1., 0.).normalize(),
      w: 0.,
    };

    mesh.subdivide_by_plane(&plane);

    mesh
      .check_is_manifold::<true>()
      .expect("Mesh should remain manifold after plane subdivision");

    assert!(mesh.faces.len() > 12);
  }
}
