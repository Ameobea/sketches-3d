//! Marching-triangles cut of a mesh along the `threshold` contour of a per-vertex scalar field:
//! straddling edges are split where the linearly interpolated field crosses, so no triangle
//! ends up on both sides, then faces are partitioned into the inside (< threshold) and outside
//! meshes. Crossings within `SNAP` of an endpoint move that vertex onto the contour instead of
//! leaving a sliver behind, except on border and seam vertices, whose coincident twin on the
//! other side of the seam would be left behind.

use fxhash::FxHashMap;
use mesh::{
  linked_mesh::{DisplacementNormalMethod, EdgeKey, EdgeSplitPos, VertexKey},
  LinkedMesh,
};

const SNAP: f32 = 0.02;

pub fn split_by_values(
  mut out: LinkedMesh<()>,
  vals: &mut FxHashMap<VertexKey, f32>,
  thr: f32,
) -> (LinkedMesh<()>, LinkedMesh<()>) {
  // Strict on both sides: a vertex sitting exactly on the contour never straddles, which is
  // also what makes the loop below terminate (every split or snap lands a vertex there).
  let crossing = |vals: &FxHashMap<VertexKey, f32>, v0: VertexKey, v1: VertexKey| {
    let (f0, f1) = (vals[&v0], vals[&v1]);
    ((f0 < thr && f1 > thr) || (f0 > thr && f1 < thr)).then(|| (thr - f0) / (f1 - f0))
  };
  // Splitting an edge re-keys its faces' other edges, so rescan until nothing straddles.
  loop {
    let straddling: Vec<EdgeKey> = out
      .edges
      .iter()
      .filter(|(_, e)| crossing(vals, e.vertices[0], e.vertices[1]).is_some())
      .map(|(k, _)| k)
      .collect();
    if straddling.is_empty() {
      break;
    }
    for ek in straddling {
      if !out.edges.contains_key(ek) {
        continue;
      }
      let [v0, v1] = out.edges[ek].vertices;
      // Low to high, so the coincident copies of a seam edge split at bit-identical points.
      let (v0, v1) = if vals[&v0] > vals[&v1] {
        (v1, v0)
      } else {
        (v0, v1)
      };
      let Some(t) = crossing(vals, v0, v1) else {
        continue;
      };
      let interior = |v: VertexKey| {
        out.vertices[v]
          .edges
          .iter()
          .all(|&e| out.edges[e].faces.len() == 2)
      };
      let snap = if t < SNAP {
        Some(v0)
      } else if t > 1. - SNAP {
        Some(v1)
      } else {
        None
      };
      if let Some(v) = snap.filter(|&v| interior(v)) {
        let (p0, p1) = (out.vertices[v0].position, out.vertices[v1].position);
        out.vertices[v].position = p0.lerp(&p1, t);
        vals.insert(v, thr);
        continue;
      }
      let vm = out.split_edge_cb(
        ek,
        EdgeSplitPos {
          pos: t,
          start_vtx_key: v0,
        },
        DisplacementNormalMethod::Interpolate,
        |_, _, _, _| {},
      );
      vals.insert(vm, thr);
    }
  }
  let mut parts = out.extract_face_partitions(2, |fk| {
    let inside = out.faces[fk].vertices.iter().any(|k| vals[k] < thr);
    if inside {
      0
    } else {
      1
    }
  });
  let outside = parts.pop().unwrap();
  let inside = parts.pop().unwrap();
  (inside, outside)
}
