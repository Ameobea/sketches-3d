use common::{maybe_init_rng, random};
use mesh::{linked_mesh::FaceKey, LinkedMesh, Mesh, Triangle};
use nalgebra::{Point3, Vector3};

pub enum MeshImpl<'a, T = ()> {
  Mesh(Mesh<'a>),
  LinkedMesh {
    mesh: &'a LinkedMesh<T>,
    face_keys: Vec<FaceKey>,
  },
}

impl<'a, T> MeshImpl<'a, T> {
  pub fn face_count(&self) -> usize {
    match self {
      MeshImpl::Mesh(mesh) => mesh.vertices.len() / 3,
      MeshImpl::LinkedMesh { mesh, .. } => mesh.faces.len(),
    }
  }

  pub fn get_face(&self, face_index: usize) -> Triangle {
    match self {
      MeshImpl::Mesh(mesh) => {
        let a = mesh.vertices[3 * face_index];
        let b = mesh.vertices[3 * face_index + 1];
        let c = mesh.vertices[3 * face_index + 2];
        Triangle::new(a, b, c)
      }
      MeshImpl::LinkedMesh { mesh, face_keys } => {
        let face_key = face_keys[face_index];
        mesh.faces[face_key].to_triangle(&mesh.vertices)
      }
    }
  }

  pub fn iter_faces(&self) -> Box<dyn Iterator<Item = Triangle> + '_> {
    match self {
      MeshImpl::Mesh(mesh) => Box::new((0..mesh.vertices.len() / 3).map(move |i| {
        let a = mesh.vertices[3 * i];
        let b = mesh.vertices[3 * i + 1];
        let c = mesh.vertices[3 * i + 2];
        Triangle::new(a, b, c)
      })),
      MeshImpl::LinkedMesh { mesh, .. } => {
        Box::new(mesh.faces.values().map(|f| f.to_triangle(&mesh.vertices)))
      }
    }
  }

  pub fn transform(&self) -> Option<nalgebra::Matrix4<f32>> {
    match self {
      MeshImpl::Mesh(mesh) => mesh.transform,
      MeshImpl::LinkedMesh { mesh, .. } => mesh.transform,
    }
  }

  fn get_normals(&self, face_index: usize, u: f32, v: f32) -> Option<Vector3<f32>> {
    let (a, b, c) = match self {
      MeshImpl::Mesh(mesh) => {
        if let Some(normals) = mesh.normals {
          let a: Vector3<_> = normals[3 * face_index];
          let b: Vector3<_> = normals[3 * face_index + 1];
          let c: Vector3<_> = normals[3 * face_index + 2];
          (a, b, c)
        } else {
          return None;
        }
      }
      MeshImpl::LinkedMesh { mesh, face_keys } => {
        let face_key = face_keys[face_index];
        let vtxs = mesh.faces[face_key].vertices;
        let Some(a) = mesh.vertices[vtxs[0]].shading_normal else {
          return None;
        };
        let Some(b) = mesh.vertices[vtxs[1]].shading_normal else {
          return None;
        };
        let Some(c) = mesh.vertices[vtxs[2]].shading_normal else {
          return None;
        };
        (a, b, c)
      }
    };

    Some(Vector3::zeros() + a.scale(u) + b.scale(v) + c.scale(1.0 - (u + v)))
  }
}

impl<'a> From<Mesh<'a>> for MeshImpl<'a> {
  fn from(mesh: Mesh<'a>) -> Self {
    MeshImpl::Mesh(mesh)
  }
}

impl<'a, T> From<&'a LinkedMesh<T>> for MeshImpl<'a, T> {
  fn from(mesh: &'a LinkedMesh<T>) -> Self {
    let face_keys: Vec<FaceKey> = mesh.faces.keys().collect();
    MeshImpl::LinkedMesh { mesh, face_keys }
  }
}

/// Largely based on https://github.com/mrdoob/three.js/blob/f8509646d78fcd4efaa4408119b55b2bead6e01b/examples/jsm/math/MeshSurfaceSampler.js
pub struct MeshSurfaceSampler<'a, T = ()> {
  mesh: MeshImpl<'a, T>,
  distribution: Vec<f32>,
}

impl<'a, T> MeshSurfaceSampler<'a, T> {
  pub fn new(mesh: impl Into<MeshImpl<'a, T>>) -> Self {
    maybe_init_rng();

    let mut samp = MeshSurfaceSampler {
      mesh: mesh.into(),
      distribution: Vec::new(),
    };

    let mut cumulative_total = 0.;
    for tri in samp.mesh.iter_faces() {
      let area = tri.area();
      cumulative_total += area;
      samp.distribution.push(cumulative_total);
    }

    samp
  }

  fn transform_matrix(&self) -> nalgebra::Matrix4<f32> {
    self
      .mesh
      .transform()
      .unwrap_or(nalgebra::Matrix4::identity())
  }

  /// Returns `(position, normal)`.
  pub fn sample(&self) -> (Point3<f32>, Vector3<f32>) {
    let face_index = self.sample_face_index();
    self.sample_face(face_index)
  }

  pub fn sample_face_index(&self) -> usize {
    let cumulative_total = *self.distribution.last().expect("distribution is empty");
    self.binary_search(random() * cumulative_total)
  }

  pub fn binary_search(&self, x: f32) -> usize {
    let mut start = 0;
    let mut end = self.distribution.len() - 1;
    let mut index = 0;
    while start <= end {
      let mid = (start + end) / 2;
      if mid == 0 || (self.distribution[mid - 1] <= x && self.distribution[mid] > x) {
        index = mid;
        break;
      } else if x < self.distribution[mid] {
        end = mid - 1;
      } else {
        start = mid + 1;
      }
    }
    index
  }

  /// Returns `(position, normal)`.
  pub fn sample_face(&self, face_index: usize) -> (Point3<f32>, Vector3<f32>) {
    let mut u = random();
    let mut v = random();
    if u + v > 1.0 {
      u = 1.0 - u;
      v = 1.0 - v;
    }

    let tri = self.mesh.get_face(face_index);

    let position: Point3<f32> =
      (tri.a.scale(u) + tri.b.scale(v) + tri.c.scale(1.0 - (u + v))).into();
    let transform = self.transform_matrix();
    let transformed_position = transform.transform_point(&position);

    let normal = self
      .mesh
      .get_normals(face_index, u, v)
      .unwrap_or_else(|| self.mesh.get_face(face_index).normal());

    // Compute transformed normal by multiplying the normal vector with the inverse
    // transpose of the 3x3 submatrix used to transform points.
    let transform_3x3 = transform
      .fixed_view::<3, 3>(0, 0)
      .try_inverse()
      .unwrap()
      .transpose();
    let transformed_normal = transform_3x3 * normal;

    (transformed_position, transformed_normal)
  }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
  let mut v = Vec::with_capacity(size);
  let ptr = v.as_mut_ptr();
  std::mem::forget(v);
  ptr
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn free(ptr: *mut u8) {
  unsafe {
    let _ = Vec::from_raw_parts(ptr, 0, 0);
  }
}
