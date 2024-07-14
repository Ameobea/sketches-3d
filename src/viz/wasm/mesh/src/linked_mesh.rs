use bitvec::{bitarr, slice::BitSlice};
use fxhash::{FxHashMap, FxHashSet};
use nalgebra::{Matrix4, Vector3};
use slotmap::{new_key_type, Key, SlotMap};
use smallvec::SmallVec;

use crate::{OwnedIndexedMesh, Triangle};

type Vec3 = Vector3<f32>;
type Mat4 = Matrix4<f32>;

new_key_type! {
  pub struct VertexKey;
  pub struct FaceKey;
  pub struct EdgeKey;
}

#[derive(Clone, Debug)]
pub struct Vertex {
  pub position: Vec3,
  /// Normal of the vertex used for shading/lighting.
  pub shading_normal: Option<Vec3>,
  /// Normal of the vertex used for displacement mapping.
  pub displacement_normal: Option<Vec3>,
  edges: Vec<EdgeKey>,
}

#[derive(Clone, Debug)]
pub struct Face {
  /// Counter-clockwise winding
  pub vertices: [VertexKey; 3],
  /// Unordered
  pub edges: [EdgeKey; 3],
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
  println!("{}", s);
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

impl Face {
  pub fn vertex_positions(&self, verts: &SlotMap<VertexKey, Vertex>) -> [Vec3; 3] {
    [
      verts[self.vertices[0]].position,
      verts[self.vertices[1]].position,
      verts[self.vertices[2]].position,
    ]
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

  pub fn compute_angle_at_vertex(
    &self,
    vtx_key: VertexKey,
    verts: &SlotMap<VertexKey, Vertex>,
  ) -> f32 {
    let vtx_ix = self
      .vertices
      .iter()
      .position(|&v| v == vtx_key)
      .unwrap_or_else(|| panic!("Vertex key {vtx_key:?} not found in face"));
    let (target_vtx_key, b, c) = match vtx_ix {
      0 => (self.vertices[0], self.vertices[1], self.vertices[2]),
      1 => (self.vertices[1], self.vertices[2], self.vertices[0]),
      2 => (self.vertices[2], self.vertices[0], self.vertices[1]),
      _ => unreachable!(),
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
#[derive(Clone)]
pub struct LinkedMesh {
  pub vertices: SlotMap<VertexKey, Vertex>,
  pub faces: SlotMap<FaceKey, Face>,
  pub edges: SlotMap<EdgeKey, Edge>,
  pub transform: Option<Mat4>,
}

fn sort_edge(v0: VertexKey, v1: VertexKey) -> [VertexKey; 2] {
  if v0 < v1 {
    [v0, v1]
  } else {
    [v1, v0]
  }
}

#[derive(Debug)]
struct NormalAcc {
  pub accumulated_normal: Vec3,
}

impl NormalAcc {
  pub fn add_face(
    &mut self,
    fan_center_vtx_key: VertexKey,
    face_key: FaceKey,
    verts: &SlotMap<VertexKey, Vertex>,
    faces: &SlotMap<FaceKey, Face>,
  ) -> Option<Vec3> {
    let face = &faces[face_key];
    let face_normal = face.normal(verts);
    if face_normal.x.is_nan() || face_normal.y.is_nan() || face_normal.z.is_nan() {
      panic!(
        "Face normal is NaN: {:?}; is_degen={}",
        face.to_triangle(verts),
        face.is_degenerate(verts)
      );
      return None;
    }

    let angle_at_vtx = face.compute_angle_at_vertex(fan_center_vtx_key, verts);
    if angle_at_vtx.is_nan() {
      panic!("NaN angle: {:?}", face.to_triangle(verts));
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
        panic!("Non-zero but still NaN normalized: {:?}", self);
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

impl LinkedMesh {
  pub fn new(vertex_count: usize, face_count: usize, transform: Option<Mat4>) -> Self {
    Self {
      vertices: SlotMap::with_capacity_and_key(vertex_count),
      faces: SlotMap::with_capacity_and_key(face_count),
      edges: SlotMap::with_key(),
      transform,
    }
  }

  pub fn iter_faces(&self) -> impl Iterator<Item = (FaceKey, &Face)> {
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
    indices: &[usize],
    normals: Option<&[Vec3]>,
    transform: Option<Mat4>,
  ) -> Self {
    let mut mesh = Self::new(vertices.len(), indices.len() / 3, transform);

    let vertex_keys_by_ix = vertices
      .iter()
      .enumerate()
      .map(|(i, &position)| {
        mesh.vertices.insert(Vertex {
          position,
          shading_normal: normals.map(|normals| normals[i]),
          displacement_normal: None,
          edges: Vec::new(),
        })
      })
      .collect::<Vec<_>>();

    for [&a_ix, &b_ix, &c_ix] in indices.iter().array_chunks::<3>() {
      let a = vertex_keys_by_ix[a_ix];
      let b = vertex_keys_by_ix[b_ix];
      let c = vertex_keys_by_ix[c_ix];

      mesh.add_face([a, b, c], [None; 3], [false; 3]);
    }

    mesh
  }

  pub fn from_triangles(triangles: &[Triangle]) -> Self {
    let mut mesh = Self::new(triangles.len() * 3, triangles.len(), None);

    for tri in triangles {
      // This might break mesh topology in some cases, but it saves us from dealing
      // with NaNs
      if tri.is_degenerate() {
        continue;
      }

      let [a_key, b_key, c_key] = [
        mesh.vertices.insert(Vertex {
          position: tri.a,
          shading_normal: None,
          displacement_normal: None,
          edges: Vec::new(),
        }),
        mesh.vertices.insert(Vertex {
          position: tri.b,
          shading_normal: None,
          displacement_normal: None,
          edges: Vec::new(),
        }),
        mesh.vertices.insert(Vertex {
          position: tri.c,
          shading_normal: None,
          displacement_normal: None,
          edges: Vec::new(),
        }),
      ];

      mesh.add_face([a_key, b_key, c_key], [None; 3], [false; 3]);
    }

    mesh
  }

  /// Removes `v1` and updates all references to it to point to `v0` instead.
  fn merge_vertices(&mut self, v0_key: VertexKey, v1_key: VertexKey) {
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
              "Triangle contains both vertices to merge: {v0_key:?}, {v1_key:?} with positions {} \
               and {}",
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

        let new_edge_vertices = sort_edge(edge.vertices[0], edge.vertices[1]);
        edge.vertices = new_edge_vertices;
        (pair_vtx_key, new_edge_vertices)
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
          vert.edges.retain(|&e| e != edge_key);
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

  pub fn merge_vertices_by_distance(&mut self, max_distance: f32) -> usize {
    // simple spatial hashing
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
              let Some(bucket) = buckets.get(&neighbor_bucket_ix) else {
                continue;
              };
              for &o_vtx_key in bucket {
                if o_vtx_key == vtx_key {
                  continue;
                }

                let o_vtx = &self.vertices[o_vtx_key];
                if distance(vtx.position, o_vtx.position) < max_distance {
                  vertices_to_merge.push(o_vtx_key);
                }
              }
            }
          }
        }
      }

      'outer: for o_vtx_key in vertices_to_merge.drain(..) {
        // if there's a triangle which contains both vertices, we can't merge them
        for &edge_key in &self.vertices[o_vtx_key].edges {
          let edge = &self.edges[edge_key];
          for face in &edge.faces {
            let face = &self.faces[*face];
            if face.vertices.contains(&vtx_key) {
              continue 'outer;
            }
          }
        }

        self.merge_vertices(vtx_key, o_vtx_key);
        removed_vert_keys.insert(o_vtx_key);
      }
    }

    removed_vert_keys.len()
  }

  pub fn debug(&self) -> String {
    fn format_vtx(key: VertexKey, _vtx: &Vertex) -> String {
      format!("{key:?}",)
    }

    fn format_edge(key: EdgeKey, edge: &Edge) -> String {
      format!("{key:?} {:?} -> {:?}", edge.vertices[0], edge.vertices[1])
    }

    fn format_face(key: FaceKey, face: &Face) -> String {
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
    indices: &[usize],
    normals: Option<&[f32]>,
    transform: Option<Mat4>,
  ) -> Self {
    let vertices =
      unsafe { std::slice::from_raw_parts(vertices.as_ptr() as *const Vec3, vertices.len() / 3) };
    let normals = if let Some(normals) = normals {
      if normals.is_empty() {
        None
      } else {
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
    let (edge_indices_to_alter, old_edge_keys, pair_vtx_keys, edge_displacement_normals) = {
      let face = &mut self.faces[face_key];
      let mut edge_indices_to_alter: [usize; 2] = [usize::MAX, usize::MAX];
      let mut pair_vtx_keys: [VertexKey; 2] = [VertexKey::null(), VertexKey::null()];
      let mut edge_displacement_normals: [Option<Vec3>; 2] = [None, None];
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
          } else {
            edge_indices_to_alter[1] = edge_ix;
            old_edge_keys[1] = edge_key;
            pair_vtx_keys[1] = if edge.vertices[0] == old_vtx_key {
              edge.vertices[1]
            } else {
              edge.vertices[0]
            };
            edge_displacement_normals[1] = edge.displacement_normal;
            break;
          }
        }
      }

      (
        edge_indices_to_alter,
        old_edge_keys,
        pair_vtx_keys,
        edge_displacement_normals,
      )
    };

    // remove this face from the old edges and delete them if they no longer
    // reference any faces
    for old_edge_key in old_edge_keys {
      let edge = &mut self.edges[old_edge_key];
      edge.faces.retain(|&mut f| f != face_key);

      if edge.faces.is_empty() {
        for vtx_key in edge.vertices {
          let vtx = &mut self.vertices[vtx_key];
          vtx.edges.retain(|&e| e != old_edge_key);
        }
        self.edges.remove(old_edge_key);
      }
    }

    let new_edge_key_0 = self.get_or_create_edge(
      [new_vtx_key, pair_vtx_keys[0]],
      edge_displacement_normals[0],
      false,
    );
    let new_edge_key_1 = self.get_or_create_edge(
      [new_vtx_key, pair_vtx_keys[1]],
      edge_displacement_normals[1],
      false,
    );
    self.edges[new_edge_key_0].faces.push(face_key);
    self.edges[new_edge_key_1].faces.push(face_key);

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
      edge.sharp |= angle > sharp_edge_threshold_rads;
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
        let edge_key_ix = vtx.edges.iter().position(|&e| e == edge_key).unwrap();
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

  /// Returns the normal of the full vertex, including all smooth fans.  That
  /// normal should be used for displacement.
  ///
  /// Can return `None` for cases like disconnected vertices or degenerate
  /// triangles.
  fn separate_and_compute_normals_for_vertex(
    &mut self,
    smooth_fans_acc: &mut Vec<SmoothFan>,
    vtx_key: VertexKey,
  ) -> Option<Vec3> {
    let mut visited_edges = bitarr![0; 1024];
    let mut visited_faces = SmallVec::<[_; 16]>::new();
    let mut smooth_fan_faces: SmallVec<[FaceKey; 16]> = SmallVec::new();

    let mut vtx_normal_acc = NormalAcc::new();

    let edge_count = self.vertices[vtx_key].edges.len();
    // keeps track of which edges have been visited.  Indices match the indices of
    // `vtx.edges`
    if edge_count > 1024 {
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
      let visited_edges = &mut visited_edges[..edge_count];

      let computed_fan_normal = self.walk_one_smooth_fan(
        vtx_key,
        visited_edges,
        &mut visited_faces,
        &mut smooth_fan_faces,
        &mut vtx_normal_acc,
      );

      let all_faces_visited = visited_edges.all();
      if all_faces_visited {
        // All faces from this fan can retain the same vertex, and we can assign
        // it the computed normal directly
        let vtx = &mut self.vertices[vtx_key];
        vtx.shading_normal = computed_fan_normal;

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
    let mut visited_edges = bitarr![0; 1024];
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
      panic!(
        "Vertex {vtx_key:?} has no displacement normal after walking smooth fans; \
         visited_faces.len()={}",
        visited_faces.len()
      );
      return None;
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

  /// Does something akin to "shade auto-smooth" from Blender.  For each edge in
  /// the mesh, it is determined to be sharp or smooth based on the angle
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
        edges: Vec::new(),
      });

      for face_key in face_keys {
        self.replace_vertex_in_face(face_key, old_key, new_vtx_key);
      }
    }
  }

  pub fn to_raw_indexed(&self) -> OwnedIndexedMesh {
    let mut vertices = Vec::with_capacity(self.vertices.len() * 3);
    let mut shading_normals = Vec::new();
    let mut displacement_normals = Vec::new();
    let mut indices = Vec::with_capacity(self.faces.len() * 3);

    let has_normals = self
      .vertices
      .values()
      .any(|vtx| vtx.shading_normal.is_some() || vtx.displacement_normal.is_some());
    if has_normals {
      shading_normals = Vec::with_capacity(self.vertices.len() * 3);
      displacement_normals = Vec::with_capacity(self.vertices.len() * 3);
    }
    let mut cur_vert_ix = 0;
    let mut seen_vertex_keys =
      FxHashMap::with_capacity_and_hasher(self.vertices.len(), fxhash::FxBuildHasher::default());

    for face in self.faces.values() {
      if face.is_degenerate(&self.vertices) {
        continue;
      }

      for &vert_key in &face.vertices {
        let vert = &self.vertices[vert_key];
        let vert_ix = *seen_vertex_keys.entry(vert_key).or_insert_with(|| {
          let ix = cur_vert_ix;
          vertices.extend(vert.position.iter());
          if let Some(shading_normal) = vert.shading_normal {
            shading_normals.extend(shading_normal.iter());
          } else if has_normals {
            // panic!("Vertex {vert_key:?} has no shading normal");
            shading_normals.extend(Vec3::zeros().iter());
          }
          if let Some(displacement_normal) = vert.displacement_normal {
            displacement_normals.extend(displacement_normal.iter());
          } else if has_normals {
            panic!("Vertex {vert_key:?} has no displacement normal");
            displacement_normals.extend(Vec3::zeros().iter());
          }
          cur_vert_ix += 1;
          ix
        });
        indices.push(vert_ix);
      }
    }

    OwnedIndexedMesh {
      vertices,
      shading_normals: if has_normals {
        Some(shading_normals)
      } else {
        None
      },
      displacement_normals: if has_normals {
        Some(displacement_normals)
      } else {
        None
      },
      indices,
      transform: self.transform.clone(),
    }
  }

  /// Splits an edge in half, creating a new vertex in the middle.  All faces
  /// that include this edge are split into two faces with new edges being
  /// created for each.
  ///
  /// Returns the key of the new vertex.
  pub fn split_edge(
    &mut self,
    edge_key_to_split: EdgeKey,
    displacement_normal_method: DisplacementNormalMethod,
  ) -> VertexKey {
    let (edge_id_to_split, edge_displacement_normal, vm_position, faces_to_split) = {
      let edge = self.edges.get(edge_key_to_split).unwrap_or_else(|| {
        panic!("Tried to split edge that doesn't exist; key={edge_key_to_split:?}")
      });
      let edge_id_to_split = edge.vertices;
      let [v0_key, v1_key] = edge_id_to_split;
      let [v0, v1] = [&self.vertices[v0_key], &self.vertices[v1_key]];

      let vm_position = (v0.position + v1.position) * 0.5;

      let faces_to_split = edge.faces.clone();

      (
        edge_id_to_split,
        edge.displacement_normal,
        vm_position,
        faces_to_split,
      )
    };

    let shading_normal = {
      let v0 = &self.vertices[edge_id_to_split[0]];
      let v1 = &self.vertices[edge_id_to_split[1]];
      v0.shading_normal
        .zip(v1.shading_normal)
        .map(|(n0, n1)| (n0 + n1).normalize())
    };

    let displacement_normal = match displacement_normal_method {
      DisplacementNormalMethod::Interpolate => {
        let v0 = &self.vertices[edge_id_to_split[0]];
        let v1 = &self.vertices[edge_id_to_split[1]];
        match v0.displacement_normal.zip(v1.displacement_normal) {
          Some((n0, n1)) => {
            let merged_normal = (n0 + n1).normalize();
            if merged_normal.x.is_nan() || merged_normal.y.is_nan() || merged_normal.z.is_nan() {
              panic!("Merged normal is NaN; n0={n0:?}; n1={n1:?}; merged_normal={merged_normal:?}");
              // Some(Vec3::new(random(), random(), random()).normalize())
              None
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
        // The other option here is average the displacement normals of the two vertices
        // of the edge being split.  However, this will cause the normals of
        // other unrelated faces to be averaged into this new vertex.  That
        // leads to a sort of inflation effect where the straight edges
        // will become curved and "blown out".
        edge_displacement_normal
      }
    };

    let middle_vertex_key = self.vertices.insert(Vertex {
      position: vm_position,
      displacement_normal,
      shading_normal,
      edges: Vec::new(),
    });

    // Split each adjacent face
    let mut new_faces = SmallVec::<[_; 32]>::new();
    for face_key in faces_to_split {
      let old_face = &self.faces[face_key];
      let old_face_normal = old_face.normal(&self.vertices);
      let [v0_key, v1_key, v2_key] = old_face.vertices;
      let edge_ids: [[VertexKey; 2]; 3] = [
        sort_edge(v0_key, v1_key),
        sort_edge(v1_key, v2_key),
        sort_edge(v2_key, v0_key),
      ];
      let edge_displacement_normals = [
        self.edges[old_face.edges[0]].displacement_normal,
        self.edges[old_face.edges[1]].displacement_normal,
        self.edges[old_face.edges[2]].displacement_normal,
      ];
      let edge_sharpnesses = [
        self.edges[old_face.edges[0]].sharp,
        self.edges[old_face.edges[1]].sharp,
        self.edges[old_face.edges[2]].sharp,
      ];
      let split_edge_ix = edge_ids
        .iter()
        .position(|&id| id == edge_id_to_split)
        .unwrap();
      let (
        (new_face_0_verts, new_face_0_edge_displacement_normals, new_face_0_edge_sharpnesses),
        (new_face_1_verts, new_face_1_edge_displacement_normals, new_face_1_edge_sharpnesses),
      ) = match split_edge_ix {
        0 => (
          (
            [v0_key, middle_vertex_key, v2_key],
            [
              edge_displacement_normals[0],
              Some(old_face_normal),
              edge_displacement_normals[2],
            ],
            [edge_sharpnesses[0], false, edge_sharpnesses[2]],
          ),
          (
            [middle_vertex_key, v1_key, v2_key],
            [
              edge_displacement_normals[0],
              edge_displacement_normals[1],
              Some(old_face_normal),
            ],
            [edge_sharpnesses[0], edge_sharpnesses[1], false],
          ),
        ),
        1 => (
          (
            [v0_key, v1_key, middle_vertex_key],
            [
              edge_displacement_normals[0],
              edge_displacement_normals[1],
              Some(old_face_normal),
            ],
            [edge_sharpnesses[0], edge_sharpnesses[1], false],
          ),
          (
            [v0_key, middle_vertex_key, v2_key],
            [
              Some(old_face_normal),
              edge_displacement_normals[1],
              edge_displacement_normals[2],
            ],
            [false, edge_sharpnesses[1], edge_sharpnesses[2]],
          ),
        ),
        2 => (
          (
            [v0_key, v1_key, middle_vertex_key],
            [
              edge_displacement_normals[0],
              Some(old_face_normal),
              edge_displacement_normals[2],
            ],
            [edge_sharpnesses[0], false, edge_sharpnesses[2]],
          ),
          (
            [v1_key, v2_key, middle_vertex_key],
            [
              edge_displacement_normals[1],
              edge_displacement_normals[2],
              Some(old_face_normal),
            ],
            [edge_sharpnesses[1], edge_sharpnesses[2], false],
          ),
        ),
        _ => unreachable!(),
      };
      new_faces.push((
        new_face_0_verts,
        new_face_0_edge_displacement_normals,
        new_face_0_edge_sharpnesses,
      ));
      new_faces.push((
        new_face_1_verts,
        new_face_1_edge_displacement_normals,
        new_face_1_edge_sharpnesses,
      ));

      self.remove_face(face_key);
    }

    for (verts, edge_displacement_normals, edge_sharpnesses) in new_faces {
      self.add_face(verts, edge_displacement_normals, edge_sharpnesses);
    }

    self.edges.remove(edge_key_to_split);

    middle_vertex_key
  }

  /// Returns the key of the edge between the two provided vertices if it exists
  /// and creates it if not.
  ///
  /// If a new edge was created, the vertices of the edge will be updated to
  /// reference it.
  fn get_or_create_edge(
    &mut self,
    vertices: [VertexKey; 2],
    displacement_normal: Option<Vec3>,
    sharp: bool,
  ) -> EdgeKey {
    let sorted_vertex_keys = sort_edge(vertices[0], vertices[1]);
    match self.vertices[vertices[0]].edges.iter().find(|&&edge_key| {
      let edge = &self.edges[edge_key];
      edge.vertices == sorted_vertex_keys
    }) {
      Some(&edge_key) => edge_key,
      None => {
        let edge_key = self.edges.insert(Edge {
          vertices: sorted_vertex_keys,
          faces: SmallVec::new(),
          sharp,
          displacement_normal,
        });

        for &vert_key in &vertices {
          let vert = &mut self.vertices[vert_key];
          vert.edges.push(edge_key);
        }

        edge_key
      }
    }
  }

  fn add_face(
    &mut self,
    vertices: [VertexKey; 3],
    edge_displacement_normals: [Option<Vec3>; 3],
    edge_sharpnesses: [bool; 3],
  ) -> Option<FaceKey> {
    let edges = [
      sort_edge(vertices[0], vertices[1]),
      sort_edge(vertices[1], vertices[2]),
      sort_edge(vertices[2], vertices[0]),
    ];
    let mut edge_keys: [EdgeKey; 3] = [EdgeKey::null(); 3];
    for (i, &[v0, v1]) in edges.iter().enumerate() {
      let edge_key =
        self.get_or_create_edge([v0, v1], edge_displacement_normals[i], edge_sharpnesses[i]);
      edge_keys[i] = edge_key;
    }

    let face_key = self.faces.insert(Face {
      vertices,
      edges: edge_keys,
    });

    for edge_key in edge_keys {
      let edge = &mut self.edges[edge_key];
      edge.faces.push(face_key);

      for vert in edge.vertices {
        let vert = &mut self.vertices[vert];
        if !vert.edges.contains(&edge_key) {
          vert.edges.push(edge_key);
        }
      }
    }

    Some(face_key)
  }

  fn remove_face(&mut self, face_key: FaceKey) {
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
      edge.faces.retain(|&mut f| f != face_key);
      if edge.faces.is_empty() {
        let edge = self.edges.remove(edge_key).unwrap_or_else(|| {
          panic!(
            "Tried to remove edge that doesn't exist; key={edge_key:?}. Was referenced by removed \
             face with key={face_key:?}",
          )
        });

        for vert_key in edge.vertices {
          let vert = &mut self.vertices[vert_key];
          vert.edges.retain(|&e| e != edge_key);
        }
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn vkey(ix: u32, version: u32) -> VertexKey {
    unsafe { std::mem::transmute((version, ix)) }
  }

  fn fkey(ix: u32, version: u32) -> FaceKey {
    unsafe { std::mem::transmute((version, ix)) }
  }

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

    let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
    let middle_edge_key = mesh
      .iter_edges()
      .find(|(_, e)| e.faces.len() == 2)
      .unwrap()
      .0;
    mesh.split_edge(middle_edge_key, DisplacementNormalMethod::EdgeNormal);

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
    let actual_indexed_mesh = mesh.to_raw_indexed();
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

    let mut mesh = LinkedMesh::from_indexed_vertices(&verts, &indices, None, None);
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
}
