//! Whole-buffer noise kernels for the texture paths, where every parameter but `pos` is
//! uniform across the buffer. Restructured octave-major over cache-sized tiles and
//! 4-wide SIMD, but bit-identical to the per-texel kernels in `noise`: same operand
//! order, same rounding, same integer hashing.

use wide::{f32x4, CmpGe, CmpGt, CmpLt};

use crate::{
  noise::{
    perm2, seed_offset_2d, seed_offset_3d, PERLIN2_SCALE, PERLIN3_SCALE, PERLIN_PERM_TABLE as PERM,
    PERM_GRAD2, PERM_GRAD3,
  },
  Vec2, Vec3,
};

const TILE: usize = 4096;
/// Above this the `f32 -> i32` cell index and the float-modulo wrap stop being exact, so
/// the tile falls back to the reference kernel.
const COORD_LIMIT: f64 = 4_194_304.;

#[derive(Clone, Copy)]
struct Oct2 {
  scale: f32,
  off: Vec2,
  amp: f32,
  /// Lattice period in cells; `0` = non-tiling.
  period: i32,
  period_f: f32,
  inv_period: f32,
}

fn octaves_2d(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  tileable: Option<f32>,
) -> Vec<Oct2> {
  let (mut freq, mut amp) = (frequency, 1f32);
  (0..octaves)
    .map(|i| {
      let (scale, period) = match tileable {
        Some(period) => {
          let cells = (period * freq).round().max(1.);
          (cells / period, cells as i32)
        }
        None => (freq, 0),
      };
      let o = Oct2 {
        scale,
        off: seed_offset_2d(seed + i as u32),
        amp,
        period,
        period_f: period as f32,
        inv_period: 1. / period as f32,
      };
      freq *= lacunarity;
      amp *= persistence;
      o
    })
    .collect()
}

#[inline(always)]
fn max_abs(s: &[f32]) -> f32 {
  s.iter().fold(0f32, |a, &v| a.max(v.abs()))
}

/// `cell.rem_euclid(period)` without the integer divide. Exact while `|cell|` stays under
/// [`COORD_LIMIT`]: `cell` and `period` are integers, `q * period` then lands within f32's
/// exact-integer range, and the two clamps absorb the at-most-one-off `q`.
#[inline(always)]
fn wrap_cells(cell: f32x4, period: f32x4, inv: f32x4) -> f32x4 {
  let w = cell - (cell * inv).floor() * period;
  let w = w.cmp_lt(f32x4::splat(0.)).blend(w + period, w);
  w.cmp_ge(period).blend(w - period, w)
}

#[inline(always)]
fn wrap_next(w: i32, period: i32) -> i32 {
  let n = w + 1;
  if n == period {
    0
  } else {
    n
  }
}

#[inline(always)]
fn xor4(a: [usize; 4], b: [usize; 4]) -> [usize; 4] {
  [a[0] ^ b[0], a[1] ^ b[1], a[2] ^ b[2], a[3] ^ b[3]]
}

#[inline(always)]
fn surflet4((gx, gy): (f32x4, f32x4), dx: f32x4, dy: f32x4) -> f32x4 {
  let attn = f32x4::splat(1.) - (dx * dx + dy * dy);
  let a4 = ((attn * attn) * attn) * attn;
  attn
    .cmp_gt(f32x4::splat(0.))
    .blend(a4 * (dx * gx + dy * gy), f32x4::splat(0.))
}

#[inline(always)]
fn gather(j: [usize; 4]) -> (f32x4, f32x4) {
  let g = &PERM_GRAD2;
  let (a, b, c, d) = (g[j[0]], g[j[1]], g[j[2]], g[j[3]]);
  (
    f32x4::from([a[0], b[0], c[0], d[0]]),
    f32x4::from([a[1], b[1], c[1], d[1]]),
  )
}

#[inline(always)]
fn v4(c: &[f32]) -> f32x4 {
  f32x4::from(<[f32; 4]>::try_from(c).unwrap())
}

/// One octave over a tile, accumulating `perlin * amp` into `out`.
fn accum_octave<const TILING: bool>(o: Oct2, xs: &[f32], ys: &[f32], out: &mut [f32]) {
  let (scale, ox, oy) = (
    f32x4::splat(o.scale),
    f32x4::splat(o.off.x),
    f32x4::splat(o.off.y),
  );
  let (kscale, kamp) = (f32x4::splat(PERLIN2_SCALE), f32x4::splat(o.amp));
  let one = f32x4::splat(1.);
  let tail = out.len() % 4;

  let mut xc = xs.chunks_exact(4);
  let mut yc = ys.chunks_exact(4);
  for oc in out.chunks_exact_mut(4) {
    let x = v4(xc.next().unwrap()) * scale + ox;
    let y = v4(yc.next().unwrap()) * scale + oy;
    let (fx, fy) = (x.floor(), y.floor());
    let (dx, dy) = (x - fx, y - fy);
    let (fdx, fdy) = (dx - one, dy - one);
    let (cx, cy) = if TILING {
      let (per, inv) = (f32x4::splat(o.period_f), f32x4::splat(o.inv_period));
      (wrap_cells(fx, per, inv), wrap_cells(fy, per, inv))
    } else {
      (fx, fy)
    };
    let nx = cx.fast_trunc_int().to_array();
    let ny = cy.fast_trunc_int().to_array();

    let (mut p0, mut p1, mut q0, mut q1) = ([0usize; 4], [0usize; 4], [0usize; 4], [0usize; 4]);
    for l in 0..4 {
      let (x1, y1) = if TILING {
        (wrap_next(nx[l], o.period), wrap_next(ny[l], o.period))
      } else {
        (nx[l] + 1, ny[l] + 1)
      };
      p0[l] = PERM[(nx[l] & 0xff) as usize] as usize;
      p1[l] = PERM[(x1 & 0xff) as usize] as usize;
      q0[l] = (ny[l] & 0xff) as usize;
      q1[l] = (y1 & 0xff) as usize;
    }

    let (g00, g10) = (gather(xor4(p0, q0)), gather(xor4(p1, q0)));
    let (g01, g11) = (gather(xor4(p0, q1)), gather(xor4(p1, q1)));
    let s = ((surflet4(g00, dx, dy) + surflet4(g10, fdx, dy)) + surflet4(g01, dx, fdy))
      + surflet4(g11, fdx, fdy);
    let acc = v4(oc) + (s * kscale) * kamp;
    oc.copy_from_slice(&acc.to_array());
  }
  let n = out.len();
  for i in n - tail..n {
    out[i] += accum_octave_scalar(o, xs[i], ys[i]);
  }
}

#[inline(always)]
fn accum_octave_scalar(o: Oct2, px: f32, py: f32) -> f32 {
  let (x, y) = (px * o.scale + o.off.x, py * o.scale + o.off.y);
  let (fx, fy) = (x.floor(), y.floor());
  let (dx, dy) = (x - fx, y - fy);
  let (fdx, fdy) = (dx - 1., dy - 1.);
  let (nx, ny) = (fx as i32, fy as i32);
  let (x0, y0, x1, y1) = if o.period != 0 {
    let (x0, y0) = (nx.rem_euclid(o.period), ny.rem_euclid(o.period));
    (x0, y0, wrap_next(x0, o.period), wrap_next(y0, o.period))
  } else {
    (nx, ny, nx + 1, ny + 1)
  };
  let p0 = PERM[(x0 & 0xff) as usize] as usize;
  let p1 = PERM[(x1 & 0xff) as usize] as usize;
  let (q0, q1) = ((y0 & 0xff) as usize, (y1 & 0xff) as usize);
  let sf = crate::noise::surflet2_at;
  let s = sf(p0 ^ q0, dx, dy) + sf(p1 ^ q0, fdx, dy) + sf(p0 ^ q1, dx, fdy) + sf(p1 ^ q1, fdx, fdy);
  (s * PERLIN2_SCALE) * o.amp
}

/// `out[i] = fbm(...pos = (xs[i], ys[i]))` for the whole buffer.
pub fn fbm_2d_batch(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  tileable: Option<f32>,
  xs: &[f32],
  ys: &[f32],
  out: &mut [f32],
) {
  let octs = octaves_2d(seed, octaves, frequency, persistence, lacunarity, tileable);
  let n = out.len();
  let mut base = 0;
  while base < n {
    let len = TILE.min(n - base);
    let (xs, ys) = (&xs[base..base + len], &ys[base..base + len]);
    let out = &mut out[base..base + len];
    let m = max_abs(xs).max(max_abs(ys)) as f64;
    out.fill(0.);
    for &o in &octs {
      if m * o.scale as f64 + 256. >= COORD_LIMIT {
        for i in 0..len {
          out[i] += reference_octave(o, xs[i], ys[i]);
        }
      } else if o.period != 0 {
        accum_octave::<true>(o, xs, ys, out);
      } else {
        accum_octave::<false>(o, xs, ys, out);
      }
    }
    base += len;
  }
}

/// Exact per-texel reference for out-of-range tiles.
fn reference_octave(o: Oct2, px: f32, py: f32) -> f32 {
  let pos = Vec2::new(px * o.scale, py * o.scale);
  let v = if o.period != 0 {
    crate::noise::periodic_perlin2_raw(pos + o.off, o.period as isize)
  } else {
    crate::noise::perlin2_raw(pos + o.off)
  };
  v * o.amp
}

#[derive(Clone, Copy)]
struct Oct3 {
  scale: f32,
  off: Vec3,
  amp: f32,
}

#[inline(always)]
fn surflet3_4(g: (f32x4, f32x4, f32x4), dx: f32x4, dy: f32x4, dz: f32x4) -> f32x4 {
  let attn = f32x4::splat(1.) - (dx * dx + dy * dy + dz * dz);
  let a4 = ((attn * attn) * attn) * attn;
  attn
    .cmp_gt(f32x4::splat(0.))
    .blend(a4 * (dx * g.0 + dy * g.1 + dz * g.2), f32x4::splat(0.))
}

#[inline(always)]
fn gather3(j: [usize; 4]) -> (f32x4, f32x4, f32x4) {
  let g = &PERM_GRAD3;
  let (a, b, c, d) = (g[j[0]], g[j[1]], g[j[2]], g[j[3]]);
  (
    f32x4::from([a[0], b[0], c[0], d[0]]),
    f32x4::from([a[1], b[1], c[1], d[1]]),
    f32x4::from([a[2], b[2], c[2], d[2]]),
  )
}

fn accum_octave3(o: Oct3, xs: &[f32], ys: &[f32], zs: &[f32], out: &mut [f32]) {
  let scale = f32x4::splat(o.scale);
  let (ox, oy, oz) = (
    f32x4::splat(o.off.x),
    f32x4::splat(o.off.y),
    f32x4::splat(o.off.z),
  );
  let (kscale, kamp) = (f32x4::splat(PERLIN3_SCALE), f32x4::splat(o.amp));
  let one = f32x4::splat(1.);
  let tail = out.len() % 4;

  let mut xc = xs.chunks_exact(4);
  let mut yc = ys.chunks_exact(4);
  let mut zc = zs.chunks_exact(4);
  for oc in out.chunks_exact_mut(4) {
    let x = v4(xc.next().unwrap()) * scale + ox;
    let y = v4(yc.next().unwrap()) * scale + oy;
    let z = v4(zc.next().unwrap()) * scale + oz;
    let (fx, fy, fz) = (x.floor(), y.floor(), z.floor());
    let (dx, dy, dz) = (x - fx, y - fy, z - fz);
    let (fdx, fdy, fdz) = (dx - one, dy - one, dz - one);
    let nx = fx.fast_trunc_int().to_array();
    let ny = fy.fast_trunc_int().to_array();
    let nz = fz.fast_trunc_int().to_array();

    let mut j = [[0usize; 4]; 8];
    for l in 0..4 {
      let (cx, cy) = (nx[l] as isize, ny[l] as isize);
      let (p00, p10) = (perm2(cx, cy), perm2(cx + 1, cy));
      let (p01, p11) = (perm2(cx, cy + 1), perm2(cx + 1, cy + 1));
      let (q0, q1) = ((nz[l] & 0xff) as usize, ((nz[l] + 1) & 0xff) as usize);
      for (k, p) in [
        p00 ^ q0,
        p10 ^ q0,
        p01 ^ q0,
        p11 ^ q0,
        p00 ^ q1,
        p10 ^ q1,
        p01 ^ q1,
        p11 ^ q1,
      ]
      .into_iter()
      .enumerate()
      {
        j[k][l] = p;
      }
    }

    let s = surflet3_4(gather3(j[0]), dx, dy, dz)
      + surflet3_4(gather3(j[1]), fdx, dy, dz)
      + surflet3_4(gather3(j[2]), dx, fdy, dz)
      + surflet3_4(gather3(j[3]), fdx, fdy, dz)
      + surflet3_4(gather3(j[4]), dx, dy, fdz)
      + surflet3_4(gather3(j[5]), fdx, dy, fdz)
      + surflet3_4(gather3(j[6]), dx, fdy, fdz)
      + surflet3_4(gather3(j[7]), fdx, fdy, fdz);
    let acc = v4(oc) + (s * kscale) * kamp;
    oc.copy_from_slice(&acc.to_array());
  }
  let n = out.len();
  for i in n - tail..n {
    let pos = Vec3::new(xs[i] * o.scale, ys[i] * o.scale, zs[i] * o.scale) + o.off;
    out[i] += crate::noise::perlin3_raw(pos) * o.amp;
  }
}

pub fn fbm_3d_batch(
  seed: u32,
  octaves: usize,
  frequency: f32,
  persistence: f32,
  lacunarity: f32,
  xs: &[f32],
  ys: &[f32],
  zs: &[f32],
  out: &mut [f32],
) {
  let (mut freq, mut amp) = (frequency, 1f32);
  let octs: Vec<Oct3> = (0..octaves)
    .map(|i| {
      let o = Oct3 {
        scale: freq,
        off: seed_offset_3d(seed + i as u32),
        amp,
      };
      freq *= lacunarity;
      amp *= persistence;
      o
    })
    .collect();

  let n = out.len();
  let mut base = 0;
  while base < n {
    let len = TILE.min(n - base);
    let (xs, ys, zs) = (
      &xs[base..base + len],
      &ys[base..base + len],
      &zs[base..base + len],
    );
    let out = &mut out[base..base + len];
    let m = max_abs(xs).max(max_abs(ys)).max(max_abs(zs)) as f64;
    out.fill(0.);
    for &o in &octs {
      if m * o.scale as f64 + 256. >= COORD_LIMIT {
        for i in 0..len {
          let pos = Vec3::new(xs[i] * o.scale, ys[i] * o.scale, zs[i] * o.scale) + o.off;
          out[i] += crate::noise::perlin3_raw(pos) * o.amp;
        }
      } else {
        accum_octave3(o, xs, ys, zs, out);
      }
    }
    base += len;
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::noise::{fbm_2d, fbm_2d_tileable, fbm_3d};

  /// The batch kernels must be bit-identical to the per-texel ones: the vectorizer picks
  /// between them freely, and every published texture golden depends on the exact bits.
  #[test]
  fn batch_matches_per_texel_kernels() {
    let (w, h) = (67, 41);
    let n = w * h;
    let mut xs = Vec::with_capacity(n);
    let mut ys = Vec::with_capacity(n);
    let mut zs = Vec::with_capacity(n);
    for y in 0..h {
      for x in 0..w {
        xs.push((x as f32 + 0.5) / w as f32 * 8. - 2.);
        ys.push((y as f32 + 0.5) / h as f32 * 8.8);
        zs.push(0.3);
      }
    }
    // The last coordinate exceeds COORD_LIMIT, forcing that tile onto the reference arm.
    xs[n - 1] = 1e30;

    let mut out = vec![0f32; n];
    for (seed, oct, freq, pers, lac) in [
      (0u32, 6usize, 1f32, 0.7f32, 2.5f32),
      (7, 5, 3., 0.5, 2.),
      (3, 1, 17., 0.5, 2.),
      (1, 0, 1., 0.5, 2.),
    ] {
      for tile in [Some(1.), Some(2.5), None] {
        fbm_2d_batch(seed, oct, freq, pers, lac, tile, &xs, &ys, &mut out);
        for i in 0..n {
          let p = Vec2::new(xs[i], ys[i]);
          let want = match tile {
            Some(t) => fbm_2d_tileable(seed, oct, freq, pers, lac, t, p),
            None => fbm_2d(seed, oct, freq, pers, lac, p),
          };
          assert_eq!(
            want.to_bits(),
            out[i].to_bits(),
            "2d {tile:?} seed={seed} i={i}"
          );
        }
      }

      fbm_3d_batch(seed, oct, freq, pers, lac, &xs, &ys, &zs, &mut out);
      for i in 0..n {
        let p = Vec3::new(xs[i], ys[i], zs[i]);
        let want = fbm_3d(seed, oct, freq, pers, lac, p);
        assert_eq!(want.to_bits(), out[i].to_bits(), "3d seed={seed} i={i}");
      }
    }
  }
}
