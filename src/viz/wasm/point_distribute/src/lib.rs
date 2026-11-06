use mesh::{linked_mesh::FaceKey, LinkedMesh, Mesh, Triangle};
use nalgebra::{Point3, Vector3};
use rand::{RngExt, SeedableRng};
use rand_pcg::Pcg32;

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
      MeshImpl::LinkedMesh { mesh, face_keys } => Box::new(
        face_keys
          .iter()
          .map(move |&face_key| mesh.faces[face_key].to_triangle(&mesh.vertices)),
      ),
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
        let a = mesh.shading_normal(vtxs[0])?;
        let b = mesh.shading_normal(vtxs[1])?;
        let c = mesh.shading_normal(vtxs[2])?;
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
    let face_keys: Vec<FaceKey> = mesh
      .faces
      .keys()
      .filter(|k| {
        let area = mesh.faces[*k].area(&mesh.vertices);
        (!area.is_nan()) && area > 0.
      })
      .collect();
    MeshImpl::LinkedMesh { mesh, face_keys }
  }
}

/// Largely based on https://github.com/mrdoob/three.js/blob/f8509646d78fcd4efaa4408119b55b2bead6e01b/examples/jsm/math/MeshSurfaceSampler.js
pub struct MeshSurfaceSampler<'a, T = ()> {
  mesh: MeshImpl<'a, T>,
  distribution: Vec<f32>,
  rng: Option<Pcg32>,
  transform: nalgebra::Matrix4<f32>,
  normal_mat: nalgebra::Matrix3<f32>,
}

impl<'a, T> MeshSurfaceSampler<'a, T> {
  /// If `rng_seed` is `None`, the shared global RNG from `common` will be used
  pub fn new(
    mesh: impl Into<MeshImpl<'a, T>>,
    rng_seed: Option<u64>,
  ) -> Result<Self, &'static str> {
    if rng_seed.is_none() {
      common::maybe_init_rng();
    }

    let mesh: MeshImpl<'a, T> = mesh.into();
    let transform = mesh.transform().unwrap_or(nalgebra::Matrix4::identity());
    let linear = transform.fixed_view::<3, 3>(0, 0).into_owned();
    let mut samp = MeshSurfaceSampler {
      normal_mat: linear.try_inverse().map_or(linear, |inv| inv.transpose()),
      transform,
      mesh,
      distribution: Vec::new(),
      rng: rng_seed.map(|seed| {
        rand_pcg::Pcg32::from_seed(unsafe {
          std::mem::transmute([seed ^ 89538u64, seed ^ 382173857842u64])
        })
      }),
    };

    let mut cumulative_total = 0.;
    for tri in samp.mesh.iter_faces() {
      let area = tri.area();
      cumulative_total += area;
      samp.distribution.push(cumulative_total);
    }

    if cumulative_total == 0. {
      return Err("MeshSurfaceSampler: mesh has no faces or all faces have zero area");
    }

    Ok(samp)
  }

  /// Returns `(position, normal)`.
  pub fn sample(&mut self) -> (Point3<f32>, Vector3<f32>) {
    let face_index = self.sample_face_index();
    self.sample_face(face_index)
  }

  fn rand(&mut self) -> &mut Pcg32 {
    if let Some(rng) = &mut self.rng {
      rng
    } else {
      common::rng()
    }
  }

  pub fn sample_face_index(&mut self) -> usize {
    let x = self.rand().random::<f32>() * self.distribution.last().unwrap();
    self
      .distribution
      .partition_point(|&d| d <= x)
      .min(self.distribution.len() - 1)
  }

  /// Returns `(position, normal)`.
  pub fn sample_face(&mut self, face_index: usize) -> (Point3<f32>, Vector3<f32>) {
    let mut u = self.rand().random();
    let mut v = self.rand().random();
    if u + v > 1. {
      u = 1. - u;
      v = 1. - v;
    }

    let tri = self.mesh.get_face(face_index);

    let position: Point3<f32> =
      (tri.a.scale(u) + tri.b.scale(v) + tri.c.scale(1. - (u + v))).into();
    let transformed_position = self.transform.transform_point(&position);

    let normal = self
      .mesh
      .get_normals(face_index, u, v)
      .unwrap_or_else(|| self.mesh.get_face(face_index).normal());

    (transformed_position, self.normal_mat * normal)
  }
}

#[test]
fn samples_are_area_weighted() {
  let tri = |x: f32, s: f32| {
    [
      Vector3::new(x, 0., 0.),
      Vector3::new(x + s, 0., 0.),
      Vector3::new(x, s, 0.),
    ]
  };
  let histogram = |sizes: &[f32]| {
    let verts: Vec<_> = sizes
      .iter()
      .enumerate()
      .flat_map(|(i, &s)| tri(10. * i as f32, s))
      .collect();
    let mesh = Mesh {
      vertices: &verts,
      normals: None,
      transform: None,
    };
    let mut samp = MeshSurfaceSampler::<()>::new(mesh, Some(7)).unwrap();
    let mut counts = vec![0usize; sizes.len()];
    for _ in 0..8000 {
      counts[samp.sample_face_index()] += 1;
    }
    counts
  };

  let counts = histogram(&[1., 4., 1., 4.]);
  for (&n, s) in counts.iter().zip([1., 16., 1., 16.]) {
    let expected = 8000. * s / 34.;
    assert!((n as f32 - expected).abs() < 0.15 * expected, "{counts:?}");
  }
  let counts = histogram(&[1., 1.]);
  assert!(counts[0].abs_diff(counts[1]) < 400, "{counts:?}");
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
