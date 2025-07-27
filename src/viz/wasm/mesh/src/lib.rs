#![feature(array_chunks, iter_array_chunks)]

use fxhash::{FxBuildHasher, FxHashMap};
use linked_mesh::{Mat4, Vec3, Vertex, VertexKey};
use nalgebra::Vector3;

pub mod linked_mesh;
pub use linked_mesh::LinkedMesh;

pub mod csg;
pub mod models;

#[derive(Clone, Debug)]
pub struct Triangle {
  pub a: Vector3<f32>,
  pub b: Vector3<f32>,
  pub c: Vector3<f32>,
}

impl Triangle {
  pub const fn new(a: Vector3<f32>, b: Vector3<f32>, c: Vector3<f32>) -> Self {
    Triangle { a, b, c }
  }

  /// Returns the normal of the triangle.
  ///
  /// Assumes the triangle's vertices are counter-clockwise.
  pub fn normal(&self) -> Vector3<f32> {
    (self.b - self.a).cross(&(self.c - self.a)).normalize()
  }

  pub fn area(&self) -> f32 {
    0.5 * self.normal().magnitude()
  }

  pub fn center(&self) -> Vector3<f32> {
    (self.a + self.b + self.c) / 3.
  }

  pub fn is_degenerate(&self) -> bool {
    if (self.a - self.b).magnitude().abs() < 1e-5
      || (self.b - self.c).magnitude().abs() < 1e-5
      || (self.c - self.a).magnitude().abs() < 1e-5
    {
      return true;
    }

    let normal = self.normal();
    if normal.x.is_nan() || normal.y.is_nan() || normal.z.is_nan() {
      return true;
    }

    false
  }
}

pub struct Mesh<'a> {
  pub vertices: &'a [Vector3<f32>],
  pub normals: Option<&'a [Vector3<f32>]>,
  pub transform: Option<nalgebra::Matrix4<f32>>,
}

impl<'a> Mesh<'a> {
  pub fn from_raw(
    vertices: &'a [f32],
    normals: &'a [f32],
    transform: Option<nalgebra::Matrix4<f32>>,
  ) -> Self {
    assert_eq!(vertices.len() % 3, 0);
    let has_normals = !normals.is_empty();
    if has_normals {
      assert_eq!(normals.len(), vertices.len());
    }

    let vertices = unsafe {
      std::slice::from_raw_parts(vertices.as_ptr() as *const Vector3<f32>, vertices.len() / 3)
    };
    let normals = if has_normals {
      Some(unsafe {
        std::slice::from_raw_parts(normals.as_ptr() as *const Vector3<f32>, normals.len() / 3)
      })
    } else {
      None
    };

    Mesh {
      vertices,
      normals,
      transform,
    }
  }
}

pub struct OwnedMesh {
  pub vertices: Vec<Vector3<f32>>,
  pub normals: Option<Vec<Vector3<f32>>>,
  pub transform: Option<nalgebra::Matrix4<f32>>,
}

impl<'a> From<Mesh<'a>> for OwnedMesh {
  fn from(mesh: Mesh<'a>) -> Self {
    OwnedMesh {
      vertices: mesh.vertices.to_vec(),
      normals: mesh.normals.map(|normals| normals.to_vec()),
      transform: mesh.transform,
    }
  }
}

impl<'a> From<&'a OwnedMesh> for Mesh<'a> {
  fn from(mesh: &'a OwnedMesh) -> Self {
    Mesh {
      vertices: &mesh.vertices,
      normals: mesh.normals.as_ref().map(|normals| normals.as_slice()),
      transform: mesh.transform,
    }
  }
}

pub struct OwnedIndexedMesh {
  pub vertices: Vec<f32>,
  pub shading_normals: Option<Vec<f32>>,
  pub displacement_normals: Option<Vec<f32>>,
  pub indices: Vec<usize>,
  pub transform: Option<nalgebra::Matrix4<f32>>,
}

pub struct OwnedIndexedMeshBuilder {
  pub cur_vert_ix: usize,
  pub seen_vtx_keys: FxHashMap<VertexKey, usize>,
  pub mesh: OwnedIndexedMesh,
}

impl OwnedIndexedMeshBuilder {
  pub fn with_capacity(
    vtx_count: usize,
    face_count: usize,
    include_displacement_normals: bool,
    include_shading_normals: bool,
  ) -> Self {
    OwnedIndexedMeshBuilder {
      cur_vert_ix: 0,
      seen_vtx_keys: FxHashMap::with_capacity_and_hasher(vtx_count, FxBuildHasher::default()),
      mesh: OwnedIndexedMesh {
        vertices: Vec::with_capacity(vtx_count * 3),
        shading_normals: if include_shading_normals {
          Some(Vec::with_capacity(vtx_count * 3))
        } else {
          None
        },
        displacement_normals: if include_displacement_normals {
          Some(Vec::with_capacity(vtx_count * 3))
        } else {
          None
        },
        indices: Vec::with_capacity(face_count * 3),
        transform: None,
      },
    }
  }

  pub fn new(include_displacement_normals: bool, include_shading_normals: bool) -> Self {
    OwnedIndexedMeshBuilder::with_capacity(
      0,
      0,
      include_displacement_normals,
      include_shading_normals,
    )
  }

  pub fn add_vtx(&mut self, vtx_key: VertexKey, vtx: &Vertex) {
    let vert_ix = *self.seen_vtx_keys.entry(vtx_key).or_insert_with(|| {
      let ix = self.cur_vert_ix;
      self.mesh.vertices.extend(vtx.position.iter());
      if let Some(shading_normals) = self.mesh.shading_normals.as_mut() {
        if let Some(shading_normal) = vtx.shading_normal {
          shading_normals.extend(shading_normal.iter());
        } else {
          // panic!("Vertex {vert_key:?} has no shading normal");
          shading_normals.extend(Vec3::zeros().iter());
        }
      }
      if let Some(displacement_normals) = self.mesh.displacement_normals.as_mut() {
        if let Some(displacement_normal) = vtx.displacement_normal {
          displacement_normals.extend(displacement_normal.iter());
        } else {
          // panic!("Vertex {vert_key:?} has no displacement normal");
          displacement_normals.extend(Vec3::zeros().iter());
        }
      }
      self.cur_vert_ix += 1;
      ix
    });
    self.mesh.indices.push(vert_ix);
  }

  pub fn build(mut self, transform: Option<Mat4>) -> OwnedIndexedMesh {
    self.mesh.transform = transform;
    self.mesh
  }

  fn is_empty(&self) -> bool {
    self.mesh.vertices.is_empty()
  }
}
