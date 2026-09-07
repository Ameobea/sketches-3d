//! Deterministic geometry experiment, using the production stitcher without changing it.
//! Run from src/viz/wasm: cargo run --offline --profile test-opt -p geoscript
//! --example fku_crease_audit -- /tmp/fku-crease-audit.json

use std::collections::BTreeSet;

use bitvec::prelude::*;
use geoscript::mesh_ops::fku_stitch::{
  dp_stitch_cost, dp_stitch_presampled, dp_stitch_solve, DpMove, Rings,
};
use mesh::linked_mesh::Vec3;
use nanoserde::SerJson;

#[derive(SerJson)]
struct ResultMesh {
  name: String,
  half_samples: usize,
  shift: f32,
  height: f32,
  slope: f32,
  dense_guides: bool,
  positions: Vec<f32>,
  indices: Vec<u32>,
  uvs: Vec<f32>,
  expected_edges: Vec<[u32; 2]>,
  crossing_triangles: usize,
  missing_crease_edges: usize,
  area: f32,
  max_surface_error: f32,
  stitch_cost: f32,
}

fn stitch(a: &[Vec3], b: &[Vec3], t: &[f32], critical: &BitSlice) -> Vec<u32> {
  let mut indices = Vec::new();
  dp_stitch_presampled(
    a,
    b,
    Some(t),
    Some(t),
    Some(critical),
    Some(critical),
    0,
    a.len(),
    false,
    &mut indices,
  );
  indices
}

fn radius(pts: &[Vec3]) -> f32 {
  let center = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
  pts.iter().map(|p| (p - center).norm()).sum::<f32>() / pts.len() as f32
}

fn anchored_span(a: &[Vec3], b: &[Vec3], t: &[f32], critical: &BitSlice, inv: f32) -> Vec<u32> {
  let mut indices = Vec::new();
  // Use the whole pair's scale and parameters in both subproblems, so splitting
  // at the anchor changes the feasible paths without changing the objective.
  for (i, j, mv) in dp_stitch_solve::<false>(Rings {
    a,
    b,
    ta: Some(t),
    tb: Some(t),
    crit_a: Some(critical),
    crit_b: Some(critical),
    inv_scale: inv,
    inv_scale_sq: inv * inv,
  }) {
    match mv {
      DpMove::AdvanceA if i > 1 => indices.extend([
        (i - 2) as u32,
        (i - 1) as u32,
        (a.len() + j.saturating_sub(1)) as u32,
      ]),
      DpMove::AdvanceB if j > 1 => indices.extend([
        i.saturating_sub(1) as u32,
        (a.len() + j - 1) as u32,
        (a.len() + j - 2) as u32,
      ]),
      _ => {}
    }
  }
  indices
}

fn experiment(
  half: usize,
  shift: f32,
  height: f32,
  slope: f32,
  dense_guides: bool,
  rows: usize,
  anchored: bool,
) -> ResultMesh {
  let n = half * 2 + 1;
  let ts: Vec<_> = (0..n).map(|i| i as f32 / (n - 1) as f32).collect();
  let a: Vec<_> = ts
    .iter()
    .map(|t| {
      let x = 2. * t - 1.;
      Vec3::new(x, slope * x.abs(), 0.)
    })
    .collect();
  let mut critical = bitvec![0; n];
  if dense_guides {
    critical.fill(true);
  }
  for i in [0, half, n - 1] {
    critical.set(i, true);
  }
  let b: Vec<_> = a.iter().map(|p| p + Vec3::new(shift, 0., height)).collect();
  let inv = 1. / ((radius(&a) + radius(&b)) * 0.5);
  let indices = if anchored {
    let mut out = Vec::new();
    for start in [0, half] {
      let end = start + half + 1;
      // Preserve original parameter values; the anchor is the only constraint change.
      let local = anchored_span(
        &a[start..end],
        &b[start..end],
        &ts[start..end],
        &critical[start..end],
        inv,
      );
      out.extend(local.into_iter().map(|i| {
        let i = i as usize;
        if i <= half {
          (start + i) as u32
        } else {
          (n + start + i - half - 1) as u32
        }
      }));
    }
    out
  } else {
    stitch(&a, &b, &ts, &critical)
  };

  let pair: Vec<_> = a.iter().chain(&b).copied().collect();
  let edges: BTreeSet<_> = indices
    .chunks_exact(3)
    .flat_map(|tri| {
      [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])].map(|(a, b)| (a.min(b), a.max(b)))
    })
    .collect();
  let missing = !edges.contains(&(half as u32, (n + half) as u32));
  let mut area = 0.;
  let mut crossing_triangles = 0;
  let mut max_surface_error = 0f32;
  let mut cost = 0.;
  for tri in indices.chunks_exact(3) {
    let p: [Vec3; 3] = std::array::from_fn(|i| pair[tri[i] as usize]);
    area += (p[1] - p[0]).cross(&(p[2] - p[0])).norm() * 0.5;
    let side = tri
      .iter()
      .map(|i| (*i as usize % n) as isize - half as isize)
      .collect::<Vec<_>>();
    if side.iter().any(|i| *i < 0) && side.iter().any(|i| *i > 0) {
      crossing_triangles += 1;
    }
    // Sample the triangle interior against y = slope*|x - shift*z/height|.
    // This is distance to the corresponding infinite supporting plane, a lower
    // bound on distance to the finite intended surface (not a Hausdorff distance).
    for i in 0..=8 {
      for j in 0..=8 - i {
        let q = (p[0] * i as f32 + p[1] * j as f32 + p[2] * (8 - i - j) as f32) / 8.;
        let error = (q.y - slope * (q.x - shift * q.z / height).abs()).abs()
          / (1. + slope * slope + (slope * shift / height).powi(2)).sqrt();
        max_surface_error = max_surface_error.max(error);
      }
    }
    // Recover the DP's advancing segment and newly closed cross edge from winding.
    let ids: [usize; 3] = std::array::from_fn(|i| tri[i] as usize);
    let (i, j, k) = if ids[0] < n && ids[1] < n {
      (ids[0], ids[1], ids[2])
    } else {
      (ids[2], ids[1], ids[0])
    };
    cost += dp_stitch_cost(
      pair[i],
      pair[j],
      pair[k],
      inv,
      inv * inv,
      ts[j % n],
      ts[k % n],
      critical[j % n] && critical[k % n],
    );
  }
  let mut positions = Vec::new();
  let mut uvs = Vec::new();
  let mut all_indices = Vec::new();
  let mut expected_edges = Vec::new();
  for row in 0..rows {
    for (i, p) in a.iter().enumerate() {
      let p = p + Vec3::new(shift, 0., height) * row as f32;
      positions.extend([p.x, p.y, p.z]);
      uvs.extend([row as f32 * height, ts[i]]);
    }
    if row + 1 < rows {
      all_indices.extend(indices.iter().map(|i| *i + (row * n) as u32));
      expected_edges.push([(row * n + half) as u32, ((row + 1) * n + half) as u32]);
    }
  }
  ResultMesh {
    name: if anchored { "anchored" } else { "production" }.to_owned(),
    half_samples: half,
    shift,
    height,
    slope,
    dense_guides,
    positions,
    indices: all_indices,
    uvs,
    expected_edges,
    crossing_triangles: crossing_triangles * (rows - 1),
    missing_crease_edges: missing as usize * (rows - 1),
    area: area * (rows - 1) as f32,
    max_surface_error,
    stitch_cost: cost,
  }
}

fn main() {
  let out = std::env::args().nth(1).expect("pass an output JSON path");
  let scan = std::env::args().any(|arg| arg == "--scan");
  if scan {
    println!(
      "half_samples,shift,height,slope,dense_guides,missing,crossing,max_surface_error,cost,\
       anchored_cost"
    );
    for half in [2, 4, 8, 16, 32, 64] {
      for slope in [0.1, 0.3, 1.] {
        for dense_guides in [false, true] {
          for height in [0.02, 0.05, 0.1, 0.2, 0.5] {
            for shift in [0.02, 0.05, 0.1, 0.2, 0.4, 0.8] {
              let mesh = experiment(half, shift, height, slope, dense_guides, 2, false);
              if mesh.missing_crease_edges == 0 {
                continue;
              }
              let control = experiment(half, shift, height, slope, dense_guides, 2, true);
              println!(
                "{half},{shift},{height},{slope},{dense_guides},{},{},{:.6},{:.6},{:.6}",
                mesh.missing_crease_edges,
                mesh.crossing_triangles,
                mesh.max_surface_error,
                mesh.stitch_cost,
                control.stitch_cost
              );
            }
          }
        }
      }
    }
  }
  // Same sparse-guide V as scripts/fku-audit/repros/planar, with rigidly
  // rotated coordinates. Just 17 vertices per row; no multiscale banding.
  let (half, shift, height, slope, dense_guides) = (8, 0.8, 0.2, 0.3, false);
  println!(
    "Selected half={half}, shift={shift}, height={height}, slope={slope}, \
     dense_guides={dense_guides}"
  );
  let meshes = vec![
    experiment(half, shift, height, slope, dense_guides, 12, false),
    experiment(half, shift, height, slope, dense_guides, 12, true),
  ];
  std::fs::write(out, meshes.serialize_json()).unwrap();
}
