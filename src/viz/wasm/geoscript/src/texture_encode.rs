//! f32 → unorm8 encoding of rendered-texture pixels for GPU materialization. Must match
//! the JS-side fallback in `proceduralTextures.ts` exactly: [0,1] clamp, round-half-up;
//! `Rgba8` replicates 1ch gray into rgb, zero-fills a missing b, a=255 unless 4ch.
//!
//! Sources are the texture's SoA planes, so encoding reads storage directly instead of
//! going through an interleaved staging copy — the interleave was costing more than the
//! packing it fed.

use crate::TextureFormat;

const ONES: [f32; 4] = [1.; 4];
const ZEROS: [f32; 4] = [0.; 4];

#[inline]
fn to8(v: f32) -> u8 {
  // saturating cast clamps to [0,255] and sends NaN to 0
  (v * 255. + 0.5) as u8
}

/// 4 pixels, one lane per channel → 16 interleaved rgba bytes.
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline]
fn pack_rgba4(r: &[f32], g: &[f32], b: &[f32], a: &[f32]) -> [u8; 16] {
  use core::arch::wasm32::*;
  unsafe {
    // trunc_sat of v*255+0.5 = round-half-up for positives; the saturating narrows send
    // negatives to 0 and overflow to 255, matching the scalar `as u8` cast
    let cv = |p: &[f32]| {
      let x = v128_load(p.as_ptr() as *const v128);
      i32x4_trunc_sat_f32x4(f32x4_add(f32x4_mul(x, f32x4_splat(255.)), f32x4_splat(0.5)))
    };
    let packed = u8x16_narrow_i16x8(
      i16x8_narrow_i32x4(cv(r), cv(g)),
      i16x8_narrow_i32x4(cv(b), cv(a)),
    );
    core::mem::transmute(u8x16_shuffle::<
      0,
      4,
      8,
      12,
      1,
      5,
      9,
      13,
      2,
      6,
      10,
      14,
      3,
      7,
      11,
      15,
    >(packed, packed))
  }
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
#[inline]
fn pack_rgba4(r: &[f32], g: &[f32], b: &[f32], a: &[f32]) -> [u8; 16] {
  core::array::from_fn(|i| to8([r, g, b, a][i % 4][i / 4]))
}

/// 8 pixels of a 2-channel texture → 16 interleaved rg bytes.
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline]
fn pack_rg8(r: &[f32], g: &[f32]) -> [u8; 16] {
  use core::arch::wasm32::*;
  unsafe {
    let cv = |p: &[f32], off: usize| {
      let x = v128_load(p.as_ptr().add(off) as *const v128);
      i32x4_trunc_sat_f32x4(f32x4_add(f32x4_mul(x, f32x4_splat(255.)), f32x4_splat(0.5)))
    };
    let packed = u8x16_narrow_i16x8(
      i16x8_narrow_i32x4(cv(r, 0), cv(r, 4)),
      i16x8_narrow_i32x4(cv(g, 0), cv(g, 4)),
    );
    core::mem::transmute(u8x16_shuffle::<
      0,
      8,
      1,
      9,
      2,
      10,
      3,
      11,
      4,
      12,
      5,
      13,
      6,
      14,
      7,
      15,
    >(packed, packed))
  }
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
#[inline]
fn pack_rg8(r: &[f32], g: &[f32]) -> [u8; 16] {
  core::array::from_fn(|i| to8(if i % 2 == 0 { r[i / 2] } else { g[i / 2] }))
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline]
fn pack16(src: &[f32]) -> [u8; 16] {
  use core::arch::wasm32::*;
  unsafe {
    let ld = |i: usize| {
      let x = v128_load(src.as_ptr().add(i) as *const v128);
      i32x4_trunc_sat_f32x4(f32x4_add(f32x4_mul(x, f32x4_splat(255.)), f32x4_splat(0.5)))
    };
    core::mem::transmute(u8x16_narrow_i16x8(
      i16x8_narrow_i32x4(ld(0), ld(4)),
      i16x8_narrow_i32x4(ld(8), ld(12)),
    ))
  }
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
#[inline]
fn pack16(src: &[f32]) -> [u8; 16] {
  core::array::from_fn(|i| to8(src[i]))
}

/// The four rgba lane sources for a texture with `planes.len()` channels: gray replicates
/// into rgb, a missing b is zero-filled, and a missing alpha is opaque. `konst` marks the
/// filled lanes, which are 4-wide splats indexed from 0 rather than per-texel.
struct RgbaLanes<'a> {
  lane: [&'a [f32]; 4],
  konst: [bool; 4],
}

impl<'a> RgbaLanes<'a> {
  fn new(planes: &[&'a [f32]]) -> Self {
    let (lane, konst) = match planes.len() {
      1 => (
        [planes[0], planes[0], planes[0], &ONES[..]],
        [false, false, false, true],
      ),
      2 => (
        [planes[0], planes[1], &ZEROS[..], &ONES[..]],
        [false, false, true, true],
      ),
      3 => (
        [planes[0], planes[1], planes[2], &ONES[..]],
        [false, false, false, true],
      ),
      _ => ([planes[0], planes[1], planes[2], planes[3]], [false; 4]),
    };
    Self { lane, konst }
  }

  /// The 4-wide source window for lane `c` at texel `i`.
  #[inline]
  fn win(&self, c: usize, i: usize) -> &'a [f32] {
    if self.konst[c] {
      self.lane[c]
    } else {
      &self.lane[c][i..]
    }
  }

  #[inline]
  fn at(&self, c: usize, i: usize) -> f32 {
    self.lane[c][if self.konst[c] { 0 } else { i }]
  }
}

/// unorm8-encode one slice's planes into `dst`, which must be exactly `n * bpp` bytes for
/// the format. Only u8 formats; float formats upload the raw f32s and never reach here.
pub fn encode_unorm8(planes: &[&[f32]], format: TextureFormat, dst: &mut [u8]) {
  let n = planes[0].len();
  match format {
    TextureFormat::R8 => {
      let src = planes[0];
      let mut i = 0;
      while i + 16 <= n {
        dst[i..i + 16].copy_from_slice(&pack16(&src[i..]));
        i += 16;
      }
      for k in i..n {
        dst[k] = to8(src[k]);
      }
    }
    TextureFormat::Rg8 => {
      let (r, g) = (
        planes[0],
        if planes.len() >= 2 {
          planes[1]
        } else {
          planes[0]
        },
      );
      let mut i = 0;
      while i + 8 <= n {
        dst[i * 2..i * 2 + 16].copy_from_slice(&pack_rg8(&r[i..], &g[i..]));
        i += 8;
      }
      for k in i..n {
        dst[k * 2] = to8(r[k]);
        dst[k * 2 + 1] = to8(g[k]);
      }
    }
    TextureFormat::Rgba8 => {
      let l = RgbaLanes::new(planes);
      // `chunks_exact_mut` walks the destination without a bounds check per block; at a
      // 10-slice 1024² stack this loop moves ~200 MB and is squarely memory-bound.
      for (i, o) in dst.chunks_exact_mut(16).enumerate() {
        let k = i * 4;
        o.copy_from_slice(&pack_rgba4(
          l.win(0, k),
          l.win(1, k),
          l.win(2, k),
          l.win(3, k),
        ));
      }
      for k in n & !3..n {
        for c in 0..4 {
          dst[k * 4 + c] = to8(l.at(c, k));
        }
      }
    }
    _ => unreachable!("float formats are not unorm8-encoded"),
  }
}

/// 4 texels x 4 lanes -> 4 interleaved rgba texels: a plain 4x4 float transpose.
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline]
fn interleave4x4(r: &[f32], g: &[f32], b: &[f32], a: &[f32], dst: &mut [f32]) {
  use core::arch::wasm32::*;
  unsafe {
    let ld = |p: &[f32]| v128_load(p.as_ptr() as *const v128);
    let (r, g, b, a) = (ld(r), ld(g), ld(b), ld(a));
    let t0 = i32x4_shuffle::<0, 4, 1, 5>(r, g);
    let t1 = i32x4_shuffle::<2, 6, 3, 7>(r, g);
    let t2 = i32x4_shuffle::<0, 4, 1, 5>(b, a);
    let t3 = i32x4_shuffle::<2, 6, 3, 7>(b, a);
    let o = dst.as_mut_ptr() as *mut v128;
    v128_store(o, i32x4_shuffle::<0, 1, 4, 5>(t0, t2));
    v128_store(o.add(1), i32x4_shuffle::<2, 3, 6, 7>(t0, t2));
    v128_store(o.add(2), i32x4_shuffle::<0, 1, 4, 5>(t1, t3));
    v128_store(o.add(3), i32x4_shuffle::<2, 3, 6, 7>(t1, t3));
  }
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
#[inline]
fn interleave4x4(r: &[f32], g: &[f32], b: &[f32], a: &[f32], dst: &mut [f32]) {
  for i in 0..4 {
    dst[i * 4] = r[i];
    dst[i * 4 + 1] = g[i];
    dst[i * 4 + 2] = b[i];
    dst[i * 4 + 3] = a[i];
  }
}

/// RGBA f32 expansion with the same channel semantics as `Rgba8` encoding. `dst` must be
/// exactly `4 * n`. Only needed for 3ch: other counts upload GPU-direct as R/RG/RGBA32F.
pub fn expand_rgba_f32(planes: &[&[f32]], dst: &mut [f32]) {
  let l = RgbaLanes::new(planes);
  let n = planes[0].len();
  let mut i = 0;
  while i + 4 <= n {
    interleave4x4(
      l.win(0, i),
      l.win(1, i),
      l.win(2, i),
      l.win(3, i),
      &mut dst[i * 4..i * 4 + 16],
    );
    i += 4;
  }
  for k in i..n {
    for c in 0..4 {
      dst[k * 4 + c] = l.at(c, k);
    }
  }
}

/// Row-major interleave of `planes` into `dst` (exactly `n * planes.len()`); the JS/GPU
/// boundary format, never used internally. Specialized per channel count so the inner loop
/// has a fixed stride.
pub fn interleave(planes: &[&[f32]], dst: &mut [f32]) {
  match planes {
    [p] => dst.copy_from_slice(p),
    [p0, p1] => {
      for (i, o) in dst.chunks_exact_mut(2).enumerate() {
        (o[0], o[1]) = (p0[i], p1[i]);
      }
    }
    [p0, p1, p2] => {
      for (i, o) in dst.chunks_exact_mut(3).enumerate() {
        (o[0], o[1], o[2]) = (p0[i], p1[i], p2[i]);
      }
    }
    [p0, p1, p2, p3] => {
      let n = p0.len();
      let mut i = 0;
      while i + 4 <= n {
        interleave4x4(
          &p0[i..],
          &p1[i..],
          &p2[i..],
          &p3[i..],
          &mut dst[i * 4..i * 4 + 16],
        );
        i += 4;
      }
      for k in i..n {
        (dst[k * 4], dst[k * 4 + 1], dst[k * 4 + 2], dst[k * 4 + 3]) = (p0[k], p1[k], p2[k], p3[k]);
      }
    }
    _ => unreachable!("textures hold 1..=4 channels"),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn enc(planes: &[&[f32]], f: TextureFormat) -> Vec<u8> {
    let bpp = match f {
      TextureFormat::R8 => 1,
      TextureFormat::Rg8 => 2,
      _ => 4,
    };
    let mut out = vec![0u8; planes[0].len() * bpp];
    encode_unorm8(planes, f, &mut out);
    out
  }

  #[test]
  fn matches_js_encode_semantics() {
    // clamp, NaN→0, round-half-up
    assert_eq!(
      enc(
        &[&[-1., f32::NAN], &[0., 1.], &[0.5, 0.001], &[2., 0.999]],
        TextureFormat::Rgba8
      ),
      vec![0, 0, 128, 255, 0, 255, 0, 255]
    );
    // 1ch → gray replicated to rgb, a=255
    assert_eq!(
      enc(&[&[0.5]], TextureFormat::Rgba8),
      vec![128, 128, 128, 255]
    );
    // 2ch → b zero-filled
    assert_eq!(
      enc(&[&[1.], &[0.5]], TextureFormat::Rgba8),
      vec![255, 128, 0, 255]
    );
    // 3ch → a=255
    assert_eq!(
      enc(&[&[0.25], &[0.5], &[1.]], TextureFormat::Rgba8),
      vec![64, 128, 255, 255]
    );
    assert_eq!(enc(&[&[0.5], &[1.]], TextureFormat::Rg8), vec![128, 255]);
    assert_eq!(enc(&[&[0.5]], TextureFormat::R8), vec![128]);
  }

  /// SIMD block + scalar remainder must agree with the per-texel scalar encode.
  #[test]
  fn simd_blocks_match_scalar_tail() {
    let mk = |off: f32, n: usize| (0..n).map(|i| off + i as f32 / 36.).collect::<Vec<_>>();
    for n in [1usize, 3, 4, 7, 8, 15, 16, 17, 37] {
      let (p0, p1, p2, p3) = (mk(0., n), mk(0.1, n), mk(0.2, n), mk(0.3, n));
      for planes in [
        vec![&p0[..]],
        vec![&p0[..], &p1[..]],
        vec![&p0[..], &p1[..], &p2[..]],
        vec![&p0[..], &p1[..], &p2[..], &p3[..]],
      ] {
        let l = RgbaLanes::new(&planes);
        let want: Vec<u8> = (0..n)
          .flat_map(|i| core::array::from_fn::<u8, 4, _>(|c| to8(l.at(c, i))))
          .collect();
        assert_eq!(enc(&planes, TextureFormat::Rgba8), want, "rgba8 n={n}");
      }
      let want: Vec<u8> = p0.iter().map(|&v| to8(v)).collect();
      assert_eq!(enc(&[&p0[..]], TextureFormat::R8), want, "r8 n={n}");
      let want: Vec<u8> = (0..n).flat_map(|i| [to8(p0[i]), to8(p1[i])]).collect();
      assert_eq!(
        enc(&[&p0[..], &p1[..]], TextureFormat::Rg8),
        want,
        "rg8 n={n}"
      );
    }
  }

  #[test]
  fn expand_and_interleave() {
    let mut out = [0f32; 8];
    expand_rgba_f32(&[&[0.25, 0.1], &[0.5, 0.2], &[0.75, 0.3]], &mut out);
    assert_eq!(out, [0.25, 0.5, 0.75, 1., 0.1, 0.2, 0.3, 1.]);
    let mut out = [0f32; 4];
    expand_rgba_f32(&[&[0.5]], &mut out);
    assert_eq!(out, [0.5, 0.5, 0.5, 1.]);
    expand_rgba_f32(&[&[0.5], &[0.25]], &mut out);
    assert_eq!(out, [0.5, 0.25, 0., 1.]);

    let mut out = [0f32; 6];
    interleave(&[&[1., 4.], &[2., 5.], &[3., 6.]], &mut out);
    assert_eq!(out, [1., 2., 3., 4., 5., 6.]);
  }
}
