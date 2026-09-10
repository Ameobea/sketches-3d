#![feature(likely_unlikely)]

use fxhash::{FxBuildHasher, FxHashMap};
use linked_mesh::{Mat4, Vec3, VertexKey};
use nalgebra::Vector3;

pub mod attrs;
pub mod linked_mesh;
pub mod occlusion;
pub use linked_mesh::LinkedMesh;
pub mod slotmap_utils;

pub mod csg;
// Baked model data stays native-only; wasm fetches it lazily via the `model_data` async dep.
#[cfg(not(target_arch = "wasm32"))]
pub mod models;
pub mod triangle_intersection;

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
      normals: mesh.normals.as_deref(),
      transform: mesh.transform,
    }
  }
}

/// One flattened passive vertex attribute: `arity` floats per exported vertex.
pub struct ExportedAttr {
  pub name: String,
  pub arity: usize,
  pub data: Vec<f32>,
}

pub struct OwnedIndexedMesh {
  pub vertices: Vec<f32>,
  pub shading_normals: Option<Vec<f32>>,
  pub displacement_normals: Option<Vec<f32>>,
  /// Every passive vertex channel, sorted by name; `uv` and `tangent` are ordinary entries.
  pub attrs: Vec<ExportedAttr>,
  pub indices: Vec<usize>,
  pub transform: Option<nalgebra::Matrix4<f32>>,
}

impl OwnedIndexedMesh {
  pub fn attr(&self, name: &str) -> Option<&ExportedAttr> {
    self.attrs.iter().find(|a| a.name == name)
  }
}

pub struct OwnedIndexedMeshBuilder {
  pub cur_vert_ix: usize,
  pub seen_vtx_keys: FxHashMap<VertexKey, usize>,
  pub mesh: OwnedIndexedMesh,
}

impl OwnedIndexedMeshBuilder {
  /// `attrs` lists `(name, arity)` per passive attribute, in the order `add_vtx` values arrive.
  pub fn with_capacity(
    vtx_count: usize,
    face_count: usize,
    include_displacement_normals: bool,
    include_shading_normals: bool,
    attrs: &[(String, usize)],
  ) -> Self {
    let buf = |lanes: usize| Vec::with_capacity(vtx_count * lanes);
    OwnedIndexedMeshBuilder {
      cur_vert_ix: 0,
      seen_vtx_keys: FxHashMap::with_capacity_and_hasher(vtx_count, FxBuildHasher::default()),
      mesh: OwnedIndexedMesh {
        vertices: buf(3),
        shading_normals: include_shading_normals.then(|| buf(3)),
        displacement_normals: include_displacement_normals.then(|| buf(3)),
        attrs: attrs
          .iter()
          .map(|(name, arity)| ExportedAttr {
            name: name.clone(),
            arity: *arity,
            data: buf(*arity),
          })
          .collect(),
        indices: Vec::with_capacity(face_count * 3),
        transform: None,
      },
    }
  }

  pub fn new(
    include_displacement_normals: bool,
    include_shading_normals: bool,
    attrs: &[(String, usize)],
  ) -> Self {
    OwnedIndexedMeshBuilder::with_capacity(
      0,
      0,
      include_displacement_normals,
      include_shading_normals,
      attrs,
    )
  }

  /// `attr_vals` is aligned with the builder's attribute list; only each attr's `arity` lanes are
  /// stored.
  pub fn add_vtx(
    &mut self,
    vtx_key: VertexKey,
    position: Vec3,
    shading_normal: Option<Vec3>,
    displacement_normal: Option<Vec3>,
    attr_vals: &[[f32; 4]],
  ) {
    let vert_ix = *self.seen_vtx_keys.entry(vtx_key).or_insert_with(|| {
      let ix = self.cur_vert_ix;
      self.mesh.vertices.extend(position.iter());
      if let Some(shading_normals) = self.mesh.shading_normals.as_mut() {
        shading_normals.extend(shading_normal.unwrap_or_else(Vec3::zeros).iter());
      }
      if let Some(displacement_normals) = self.mesh.displacement_normals.as_mut() {
        displacement_normals.extend(displacement_normal.unwrap_or_else(Vec3::zeros).iter());
      }
      for (attr, v) in self.mesh.attrs.iter_mut().zip(attr_vals) {
        attr.data.extend_from_slice(&v[..attr.arity]);
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
