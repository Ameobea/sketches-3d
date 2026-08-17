//! Point-set sampling primitives. `poisson_points_2d` generates a blue-noise (Poisson-disk)
//! point set over [0,1)² with a TOROIDAL distance metric, so the set tiles seamlessly with
//! `repeat`-wrapped textures. Bridson's algorithm; explicit seed keeps it pure.

use std::rc::Rc;

use fxhash::FxHashMap;
use rand::{RngExt, SeedableRng};
use rand_pcg::Pcg32;

use crate::{seq::EagerSeq, ArgRef, ErrorStack, Sym, Value, Vec2};

fn torus_d2(a: Vec2, b: Vec2) -> f32 {
  let mut dx = (a.x - b.x).abs();
  let mut dy = (a.y - b.y).abs();
  if dx > 0.5 {
    dx = 1. - dx;
  }
  if dy > 0.5 {
    dy = 1. - dy;
  }
  dx * dx + dy * dy
}

fn bridson_torus(r: f32, rng: &mut Pcg32) -> Vec<Vec2> {
  let cell = r / std::f32::consts::SQRT_2;
  let gw = ((1. / cell).ceil() as usize).max(1);
  let mut grid: Vec<u32> = vec![u32::MAX; gw * gw];
  let mut pts: Vec<Vec2> = Vec::new();
  let mut active: Vec<u32> = Vec::new();

  let cell_ix = |p: Vec2| -> usize {
    let cx = ((p.x * gw as f32) as usize).min(gw - 1);
    let cy = ((p.y * gw as f32) as usize).min(gw - 1);
    cy * gw + cx
  };
  let has_neighbor = |p: Vec2, grid: &[u32], pts: &[Vec2]| -> bool {
    let cx = ((p.x * gw as f32) as i64).min(gw as i64 - 1);
    let cy = ((p.y * gw as f32) as i64).min(gw as i64 - 1);
    for oy in -2i64..=2 {
      for ox in -2i64..=2 {
        let gx = (cx + ox).rem_euclid(gw as i64) as usize;
        let gy = (cy + oy).rem_euclid(gw as i64) as usize;
        let ix = grid[gy * gw + gx];
        if ix != u32::MAX && torus_d2(p, pts[ix as usize]) < r * r {
          return true;
        }
      }
    }
    false
  };

  let p0 = Vec2::new(rng.random::<f32>(), rng.random::<f32>());
  grid[cell_ix(p0)] = 0;
  pts.push(p0);
  active.push(0);

  while !active.is_empty() {
    let ai = rng.random_range(0..active.len());
    let p = pts[active[ai] as usize];
    let mut found = false;
    for _ in 0..30 {
      let ang = rng.random::<f32>() * std::f32::consts::TAU;
      let d = r * (1. + rng.random::<f32>());
      // f32 rem_euclid can round to exactly 1.0; snap to keep the [0,1) contract
      let wrap = |v: f32| {
        let r = v.rem_euclid(1.);
        if r >= 1. {
          0.
        } else {
          r
        }
      };
      let q = Vec2::new(wrap(p.x + ang.cos() * d), wrap(p.y + ang.sin() * d));
      if !has_neighbor(q, &grid, &pts) {
        let ix = pts.len() as u32;
        grid[cell_ix(q)] = ix;
        pts.push(q);
        active.push(ix);
        found = true;
        break;
      }
    }
    if !found {
      active.swap_remove(ai);
    }
  }

  pts
}

pub(crate) fn poisson_points_2d_impl(
  def_ix: usize,
  arg_refs: &[ArgRef],
  args: &[Value],
  kwargs: &FxHashMap<Sym, Value>,
) -> Result<Value, ErrorStack> {
  let seed = arg_refs[1].resolve(args, kwargs).as_int().unwrap();
  let mut rng = Pcg32::seed_from_u64(seed as u64);

  let pts = match def_ix {
    0 => {
      let r = arg_refs[0].resolve(args, kwargs).as_float().unwrap();
      // The floor mirrors the count form's 1M-point cap (n ~ 0.6963/r²); without it, grid
      // allocation is O(1/r²) and `gw*gw` can even wrap 32-bit usize on wasm.
      if !(r >= 8e-4 && r < 1.) {
        return Err(ErrorStack::new(format!(
          "Invalid `poisson_points_2d` radius: {r}; expected 0.0008 <= radius < 1 (in UV units)"
        )));
      }
      bridson_torus(r, &mut rng)
    }
    1 => {
      let n = arg_refs[0].resolve(args, kwargs).as_int().unwrap();
      if !(1..=1_000_000).contains(&n) {
        return Err(ErrorStack::new(format!(
          "Invalid `poisson_points_2d` count: {n}; expected 1..=1000000"
        )));
      }
      let n = n as usize;
      // Bridson fills to ~0.6963/r² points on the unit torus; overshoot, then shrink the
      // radius until the target is reached. A shuffled subset of a Poisson set keeps the
      // min-distance guarantee (truncating the raw BFS-ordered output would bias spatially).
      let mut r = (0.6963 / (1.15 * n as f32)).sqrt().min(0.45);
      let mut pts = loop {
        let pts = bridson_torus(r, &mut rng);
        if pts.len() >= n {
          break pts;
        }
        r *= 0.85;
      };
      for i in (1..pts.len()).rev() {
        pts.swap(i, rng.random_range(0..=i));
      }
      pts.truncate(n);
      pts
    }
    _ => unimplemented!(),
  };

  Ok(Value::Sequence(Rc::new(EagerSeq {
    inner: Rc::new(pts.into_iter().map(Value::Vec2).collect::<Vec<_>>()),
  })))
}

#[cfg(test)]
mod tests {
  use super::torus_d2;
  use crate::{parse_and_eval_program, Value, Vec2};

  fn get_points(ctx: &crate::EvalCtx, name: &str) -> Vec<Vec2> {
    let Value::Sequence(seq) = ctx.get_global(name).unwrap() else {
      panic!("Expected {name} to be a sequence");
    };
    seq
      .consume(ctx)
      .map(|v| match v.unwrap() {
        Value::Vec2(p) => p,
        other => panic!("Expected vec2, found: {other:?}"),
      })
      .collect()
  }

  #[test]
  fn poisson_points_2d_props() {
    let ctx = parse_and_eval_program(
      r#"
by_radius = poisson_points_2d(0.15, seed=1)
by_radius_same = poisson_points_2d(0.15, seed=1)
by_radius_other = poisson_points_2d(0.15, seed=2)
by_count = poisson_points_2d(40)
"#,
    )
    .unwrap();

    let pts = get_points(&ctx, "by_radius");
    assert!(
      (15..=45).contains(&pts.len()),
      "unexpected point count {}",
      pts.len()
    );
    for i in 0..pts.len() {
      for j in (i + 1)..pts.len() {
        let d2 = torus_d2(pts[i], pts[j]);
        assert!(
          d2 >= 0.15 * 0.15 - 1e-6,
          "points {i}/{j} too close: toroidal dist {}",
          d2.sqrt()
        );
      }
      assert!(pts[i].x >= 0. && pts[i].x < 1. && pts[i].y >= 0. && pts[i].y < 1.);
    }

    assert_eq!(pts, get_points(&ctx, "by_radius_same"), "seed determinism");
    assert_ne!(pts, get_points(&ctx, "by_radius_other"), "seed variation");

    let by_count = get_points(&ctx, "by_count");
    assert_eq!(by_count.len(), 40);
    for i in 0..by_count.len() {
      for j in (i + 1)..by_count.len() {
        assert!(torus_d2(by_count[i], by_count[j]) > 1e-8, "duplicate point");
      }
    }
  }
}
