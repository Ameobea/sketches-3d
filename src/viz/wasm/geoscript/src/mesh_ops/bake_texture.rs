//! UV-space rasterization of a mesh: which triangle, with which barycentric weights, covers each
//! texel of a `w`×`h` atlas over the mesh's `uv` channel. Coverage is padded outward by `dilate`
//! texels (each ring copies the nearest covered texel's assignment) so bilinear taps and mips
//! at island borders never bleed background in.

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{Vec3, VertexKey},
  LinkedMesh,
};

use crate::{tex_mem, ErrorStack};

const BACKGROUND: u32 = u32::MAX;
const EDGE_EPS: f32 = 1e-4;

pub struct UvRaster {
  pub w: usize,
  pub h: usize,
  pub verts: Vec<VertexKey>,
  /// Indices into `verts`.
  pub faces: Vec<[u32; 3]>,
  pub face: Vec<u32>,
  /// Weights of the face's first two corners; the third is the remainder.
  pub bary: Vec<[f32; 2]>,
  _charge: tex_mem::Charge,
  /// Local-space edge length of one texel inside each face (sqrt of the area ratio), for
  /// footprint-aware sampling.
  pub face_texel: Vec<f32>,
}

impl UvRaster {
  pub fn new(mesh: &LinkedMesh<()>, w: usize, h: usize, dilate: usize) -> Result<Self, ErrorStack> {
    let uv = mesh.vertex_channels.get("uv").ok_or_else(|| {
      ErrorStack::new(
        "Mesh has no `uv` attribute to bake over; run `compute_uvs` (or `rail_sweep`) first",
      )
    })?;
    let mut verts: Vec<VertexKey> = Vec::new();
    let mut vert_ix: FxHashMap<VertexKey, u32> = FxHashMap::default();
    let mut ix_of = |k: VertexKey| {
      *vert_ix.entry(k).or_insert_with(|| {
        verts.push(k);
        (verts.len() - 1) as u32
      })
    };
    let faces: Vec<[u32; 3]> = mesh
      .faces
      .values()
      .map(|f| f.vertices.map(&mut ix_of))
      .collect();
    let n = w * h;
    let mut face = tex_mem::try_vec(n, BACKGROUND)?;
    let mut bary = tex_mem::try_vec(n, [0f32; 2])?;
    let mut face_texel = vec![0f32; faces.len()];
    let (fw, fh) = (w as f32, h as f32);
    for (fi, tri) in faces.iter().enumerate() {
      let keys = tri.map(|i| verts[i as usize]);
      let p = keys.map(|k| {
        let v = uv.get(k).unwrap_or_default();
        [v[0] * fw - 0.5, v[1] * fh - 0.5]
      });
      let area =
        (p[1][0] - p[0][0]) * (p[2][1] - p[0][1]) - (p[2][0] - p[0][0]) * (p[1][1] - p[0][1]);
      if area.abs() < 1e-12 {
        continue;
      }
      let [a, b, c] = keys.map(|k| mesh.vertices[k].position);
      face_texel[fi] = ((b - a).cross(&(c - a)).norm() / area.abs()).sqrt();
      let inv = 1. / area;
      let lo = |ax: usize| {
        p.iter()
          .map(|q| q[ax])
          .fold(f32::INFINITY, f32::min)
          .ceil()
          .max(0.)
      };
      let hi = |ax: usize, n: f32| {
        p.iter()
          .map(|q| q[ax])
          .fold(f32::NEG_INFINITY, f32::max)
          .floor()
          .min(n - 1.)
      };
      let (x0, x1, y0, y1) = (lo(0), hi(0, fw), lo(1), hi(1, fh));
      if x1 < x0 || y1 < y0 {
        continue;
      }
      for y in y0 as usize..=y1 as usize {
        for x in x0 as usize..=x1 as usize {
          let (px, py) = (x as f32, y as f32);
          let w0 = ((p[1][0] - px) * (p[2][1] - py) - (p[2][0] - px) * (p[1][1] - py)) * inv;
          let w1 = ((p[2][0] - px) * (p[0][1] - py) - (p[0][0] - px) * (p[2][1] - py)) * inv;
          let w2 = 1. - w0 - w1;
          if w0 >= -EDGE_EPS && w1 >= -EDGE_EPS && w2 >= -EDGE_EPS {
            let i = y * w + x;
            face[i] = fi as u32;
            bary[i] = [w0, w1];
          }
        }
      }
    }
    let mut r = Self {
      w,
      h,
      verts,
      faces,
      face,
      bary,
      face_texel,
      _charge: tex_mem::Charge::new("bake atlas", n * Self::TEXEL_BYTES),
    };
    r.dilate(dilate);
    Ok(r)
  }

  fn dilate(&mut self, rings: usize) {
    const N8: [(isize, isize); 8] = [
      (-1, -1),
      (0, -1),
      (1, -1),
      (-1, 0),
      (1, 0),
      (-1, 1),
      (0, 1),
      (1, 1),
    ];
    let (w, h) = (self.w as isize, self.h as isize);
    let around = move |i: usize| {
      let (x, y) = (i as isize % w, i as isize / w);
      N8.iter()
        .map(move |(dx, dy)| (x + dx, y + dy))
        .filter(move |&(nx, ny)| nx >= 0 && ny >= 0 && nx < w && ny < h)
        .map(move |(nx, ny)| (ny * w + nx) as usize)
    };
    // Ring in which each texel got its assignment; a ring only grows from earlier ones.
    let mut gen: Vec<u8> = self
      .face
      .iter()
      .map(|&f| if f == BACKGROUND { u8::MAX } else { 0 })
      .collect();
    let mut queued = vec![false; gen.len()];
    let mut grow = |from: &[usize], gen: &[u8]| {
      let mut next: Vec<usize> = Vec::new();
      for &i in from {
        for j in around(i) {
          if gen[j] == u8::MAX && !queued[j] {
            queued[j] = true;
            next.push(j);
          }
        }
      }
      next
    };
    let covered: Vec<usize> = (0..gen.len()).filter(|&i| gen[i] == 0).collect();
    let mut frontier = grow(&covered, &gen);
    drop(covered);
    for ring in 1..=rings.min(u8::MAX as usize - 1) as u8 {
      if frontier.is_empty() {
        break;
      }
      for &i in &frontier {
        let j = around(i).find(|&j| gen[j] < ring).unwrap();
        self.face[i] = self.face[j];
        self.bary[i] = self.bary[j];
      }
      for &i in &frontier {
        gen[i] = ring;
      }
      frontier = grow(&frontier, &gen);
    }
  }

  pub const TEXEL_BYTES: usize = size_of::<u32>() + size_of::<[f32; 2]>();

  /// `per_vertex` (indexed like `verts`) interpolated at texel `i`, which must be covered.
  #[inline(always)]
  pub fn lerp(&self, i: usize, per_vertex: &[Vec3]) -> Vec3 {
    let [a, b, c] = self.faces[self.face[i] as usize].map(|v| per_vertex[v as usize]);
    let [u, v] = self.bary[i];
    a * u + b * v + c * (1. - u - v)
  }

  pub fn covered(&self, i: usize) -> bool {
    self.face[i] != BACKGROUND
  }

  pub fn texel_size(&self, i: usize) -> f32 {
    self.face_texel[self.face[i] as usize]
  }

  /// One plane per lane, interpolating `per_vertex` with the texel's barycentrics; background
  /// texels are 0.
  pub fn interpolate(
    &self,
    lanes: usize,
    per_vertex: impl Fn(VertexKey) -> [f32; 4],
  ) -> Result<Vec<Vec<f32>>, ErrorStack> {
    let vals: Vec<[f32; 4]> = self.verts.iter().map(|&k| per_vertex(k)).collect();
    let n = self.w * self.h;
    let mut planes = tex_mem::try_planes(lanes, n)?;
    for i in 0..n {
      if !self.covered(i) {
        continue;
      }
      let [a, b, c] = self.faces[self.face[i] as usize].map(|v| vals[v as usize]);
      let [u, v] = self.bary[i];
      for (l, plane) in planes.iter_mut().enumerate() {
        plane[i] = u * a[l] + v * b[l] + (1. - u - v) * c[l];
      }
    }
    Ok(planes)
  }
}
