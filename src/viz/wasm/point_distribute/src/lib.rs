use common::{maybe_init_rng, random};
use mesh::{Mesh, Triangle};
use nalgebra::{Point3, Vector3};

/// Largely based on https://github.com/mrdoob/three.js/blob/f8509646d78fcd4efaa4408119b55b2bead6e01b/examples/jsm/math/MeshSurfaceSampler.js
pub struct MeshSurfaceSampler<'a> {
  mesh: Mesh<'a>,
  distribution: Vec<f32>,
}

impl<'a> MeshSurfaceSampler<'a> {
  pub fn new(mesh: Mesh<'a>) -> Self {
    maybe_init_rng();

    if let Some(normals) = mesh.normals.as_ref() {
      assert_eq!(mesh.vertices.len(), normals.len());
    }

    let mut samp = MeshSurfaceSampler {
      mesh,
      distribution: Vec::new(),
    };

    let total_faces = samp.mesh.vertices.len() / 3;
    let mut face_weights = Vec::with_capacity(total_faces);

    for i in 0..total_faces {
      let a = samp.mesh.vertices[3 * i];
      let b = samp.mesh.vertices[3 * i + 1];
      let c = samp.mesh.vertices[3 * i + 2];

      let triangle = Triangle::new(a, b, c);
      face_weights.push(triangle.area());
    }

    let mut cumulative_total = 0.0;
    for i in 0..total_faces {
      cumulative_total += face_weights[i];
      samp.distribution.push(cumulative_total);
    }

    samp
  }

  fn transform_matrix(&self) -> nalgebra::Matrix4<f32> {
    self.mesh.transform.unwrap_or(nalgebra::Matrix4::identity())
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

    let (a, b, c) = (
      self.mesh.vertices[3 * face_index],
      self.mesh.vertices[3 * face_index + 1],
      self.mesh.vertices[3 * face_index + 2],
    );

    let position: Point3<f32> = (a.scale(u) + b.scale(v) + c.scale(1.0 - (u + v))).into();
    let transform = self.transform_matrix();
    let transformed_position = transform.transform_point(&position);

    let normal = match self.mesh.normals.as_ref() {
      Some(normals) => {
        let a: Vector3<_> = normals[3 * face_index];
        let b: Vector3<_> = normals[3 * face_index + 1];
        let c: Vector3<_> = normals[3 * face_index + 2];
        (Vector3::zeros() + a.scale(u) + b.scale(v) + c.scale(1.0 - (u + v))).normalize()
      }
      None => Triangle::new(a, b, c).normal(),
    };

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

#[no_mangle]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
  let mut v = Vec::with_capacity(size);
  let ptr = v.as_mut_ptr();
  std::mem::forget(v);
  ptr
}

#[no_mangle]
pub extern "C" fn free(ptr: *mut u8) {
  unsafe {
    let _ = Vec::from_raw_parts(ptr, 0, 0);
  }
}
