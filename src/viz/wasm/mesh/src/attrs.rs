//! Well-known per-vertex attribute names and the propagation policy each carries.

use slotmap::SecondaryMap;

use crate::linked_mesh::{Arity, Channel, FlipXform, Interp, SpatialXform, VertexKey};
use crate::LinkedMesh;

pub const UV: &str = "uv";
pub const TANGENT: &str = "tangent";
pub const COLOR: &str = "color";

pub struct AttrSpec {
  pub name: &'static str,
  pub arities: &'static [Arity],
  pub interp: Interp,
  pub flip: FlipXform,
  pub spatial: SpatialXform,
  /// Exported to the GPU without being listed in `render(attrs=)`.
  pub export_default: bool,
}

pub const KNOWN: &[AttrSpec] = &[
  AttrSpec {
    name: UV,
    arities: &[Arity::Vec2],
    interp: Interp::Lerp,
    flip: FlipXform::Identity,
    spatial: SpatialXform::Identity,
    export_default: true,
  },
  AttrSpec {
    name: TANGENT,
    arities: &[Arity::Vec4],
    interp: Interp::Lerp,
    flip: FlipXform::Negate,
    spatial: SpatialXform::Direction,
    export_default: true,
  },
  AttrSpec {
    name: COLOR,
    arities: &[Arity::Vec3, Arity::Vec4],
    interp: Interp::Lerp,
    flip: FlipXform::Identity,
    spatial: SpatialXform::Identity,
    export_default: true,
  },
];

/// Lanes an attribute occupies in the exported vertex buffer. `tangent` is always padded to
/// glTF's vec4 so a Vec3 channel still carries a handedness lane.
pub fn export_arity(name: &str, arity: Arity) -> usize {
  if name == TANGENT {
    4
  } else {
    match arity {
      Arity::Scalar => 1,
      Arity::Vec2 => 2,
      Arity::Vec3 => 3,
      Arity::Vec4 => 4,
    }
  }
}

/// Export value for one vertex; missing tangents become the identity frame and tangents with an
/// unset handedness lane get `w = 1`, everything else zero-fills.
pub fn export_value(name: &str, v: Option<[f32; 4]>) -> [f32; 4] {
  if name == TANGENT {
    v.map_or([0., 0., 0., 1.], |v| {
      [v[0], v[1], v[2], if v[3] != 0. { v[3] } else { 1. }]
    })
  } else {
    v.unwrap_or_default()
  }
}

pub fn known(name: &str) -> Option<&'static AttrSpec> {
  KNOWN.iter().find(|s| s.name == name)
}

impl Channel<VertexKey> {
  /// Channel carrying the registry policy for `name`; custom names get the passive default.
  pub fn for_attr(name: &str, arity: Arity) -> Self {
    match known(name) {
      Some(s) => Channel::new(arity, s.interp, s.flip, s.spatial),
      None => Channel::new(
        arity,
        Interp::Lerp,
        FlipXform::Identity,
        SpatialXform::Identity,
      ),
    }
  }
}

pub fn uv_channel() -> Channel<VertexKey> {
  Channel::for_attr(UV, Arity::Vec2)
}

pub fn tangent_channel() -> Channel<VertexKey> {
  Channel::for_attr(TANGENT, Arity::Vec4)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SmoothWeights {
  Uniform,
  Cotan,
}

impl<T> LinkedMesh<T> {
  /// Jacobi-relaxes channel `name` over one-rings: every vertex moves `lambda` of the way toward
  /// its neighbours' weighted mean, `iterations` times. Direction / normalizing channels are
  /// renormalized each pass. Vertices whose `pin` channel value is nonzero stay fixed.
  pub fn smooth_channel(
    &mut self,
    name: &str,
    iterations: usize,
    lambda: f32,
    weights: SmoothWeights,
    pin: Option<&str>,
  ) {
    let Some(ch) = self.vertex_channels.get(name) else {
      return;
    };
    let keys: Vec<VertexKey> = self.vertices.keys().collect();
    let key_ix: SecondaryMap<VertexKey, usize> =
      keys.iter().enumerate().map(|(i, &k)| (k, i)).collect();
    let rings: Vec<Vec<(usize, f32)>> = keys
      .iter()
      .map(|&k| {
        self.vertices[k]
          .edges
          .iter()
          .map(|&ek| {
            let e = &self.edges[ek];
            let w = match weights {
              SmoothWeights::Uniform => 1.,
              // half the sum of the cotangents of the angles opposite the edge; clamped so
              // obtuse and degenerate triangles can't push weights negative or to infinity
              SmoothWeights::Cotan => {
                e.faces
                  .iter()
                  .map(|&fk| {
                    let f = &self.faces[fk];
                    let opp = *f
                      .vertices
                      .iter()
                      .find(|&&v| v != e.vertices[0] && v != e.vertices[1])
                      .unwrap();
                    (1. / f.compute_angle_at_vertex_key(opp, &self.vertices).tan()).clamp(0., 1e4)
                  })
                  .sum::<f32>()
                  * 0.5
              }
            };
            (key_ix[e.other_vtx(k)], w)
          })
          .collect()
      })
      .collect();

    let pinned: Vec<bool> = match pin.and_then(|p| self.vertex_channels.get(p)) {
      Some(p) => keys
        .iter()
        .map(|&k| p.get(k).is_some_and(|v| v[0] != 0.))
        .collect(),
      None => vec![false; keys.len()],
    };
    let renormalize = ch.interp == Interp::LerpNormalize || ch.spatial == SpatialXform::Direction;
    let lanes = ch.store.arity();
    let mut cur: Vec<Option<[f32; 4]>> = keys.iter().map(|&k| ch.get(k)).collect();

    for _ in 0..iterations {
      cur = (0..keys.len())
        .map(|i| {
          let x = cur[i]?;
          if pinned[i] {
            return Some(x);
          }
          let (mut acc, mut wsum) = ([0f32; 4], 0f32);
          for &(j, w) in &rings[i] {
            if let Some(y) = cur[j] {
              for l in 0..4 {
                acc[l] += w * y[l];
              }
              wsum += w;
            }
          }
          if wsum <= 0. {
            return Some(x);
          }
          let mut out = [0f32; 4];
          for l in 0..4 {
            out[l] = x[l] + lambda * (acc[l] / wsum - x[l]);
          }
          if renormalize {
            let n = out[..lanes].iter().map(|v| v * v).sum::<f32>().sqrt();
            if n > 1e-12 {
              out[..lanes].iter_mut().for_each(|v| *v /= n);
            }
          }
          Some(out)
        })
        .collect();
    }

    let ch = self.vertex_channels.get_mut(name).unwrap();
    for (i, &k) in keys.iter().enumerate() {
      if let Some(v) = cur[i] {
        ch.set(k, v);
      }
    }
  }
}
