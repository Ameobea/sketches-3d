#![feature(iter_array_chunks)]

use nalgebra::Vector3;

pub mod linked_mesh;
pub use linked_mesh::LinkedMesh;

#[derive(Debug)]
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
    let mag = self.normal().magnitude();
    mag == 0. || mag.is_nan()
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

pub struct OwnedIndexedMesh {
  pub vertices: Vec<f32>,
  pub shading_normals: Option<Vec<f32>>,
  pub displacement_normals: Option<Vec<f32>>,
  pub indices: Vec<usize>,
  pub transform: Option<nalgebra::Matrix4<f32>>,
}
