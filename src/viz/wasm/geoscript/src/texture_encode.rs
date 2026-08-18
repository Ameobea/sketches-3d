//! f32 → unorm8 encoding of rendered-texture pixels for GPU materialization. Must match
//! the JS-side fallback in `proceduralTextures.ts` exactly: [0,1] clamp, round-half-up;
//! `Rgba8` replicates 1ch gray into rgb, zero-fills a missing b, a=255 unless 4ch.

use crate::TextureFormat;

#[inline]
fn to8(v: f32) -> u8 {
  // saturating cast clamps to [0,255] and sends NaN to 0
  (v * 255. + 0.5) as u8
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[inline]
fn pack16(src: &[f32; 16]) -> [u8; 16] {
  use core::arch::wasm32::*;
  unsafe {
    let ld = |i: usize| {
      f32x4_add(
        f32x4_mul(v128_load(src.as_ptr().add(i) as *const v128), f32x4_splat(255.)),
        f32x4_splat(0.5),
      )
    };
    // trunc_sat of v*255+0.5 = round-half-up for positives; the saturating narrows send
    // negatives to 0 and overflow to 255, matching the scalar `as u8` cast
    let a = i32x4_trunc_sat_f32x4(ld(0));
    let b = i32x4_trunc_sat_f32x4(ld(4));
    let c = i32x4_trunc_sat_f32x4(ld(8));
    let d = i32x4_trunc_sat_f32x4(ld(12));
    core::mem::transmute(u8x16_narrow_i16x8(
      i16x8_narrow_i32x4(a, b),
      i16x8_narrow_i32x4(c, d),
    ))
  }
}

#[cfg(not(all(target_arch = "wasm32", target_feature = "simd128")))]
#[inline]
fn pack16(src: &[f32; 16]) -> [u8; 16] {
  core::array::from_fn(|i| to8(src[i]))
}

fn pack_contiguous(px: &[f32], out: &mut Vec<u8>) {
  let mut chunks = px.chunks_exact(16);
  for ch in &mut chunks {
    out.extend_from_slice(&pack16(ch.try_into().unwrap()));
  }
  out.extend(chunks.remainder().iter().map(|&v| to8(v)));
}

/// Encode one slice's interleaved f32 pixels into `out`. Only u8 formats; float formats
/// upload the raw f32s and never reach here.
pub fn encode_unorm8(px: &[f32], channels: usize, format: TextureFormat, out: &mut Vec<u8>) {
  let c = channels;
  let take = match format {
    TextureFormat::R8 => 1,
    TextureFormat::Rg8 => 2,
    TextureFormat::Rgba8 => 4,
    _ => unreachable!("float formats are not unorm8-encoded"),
  };
  if take == c {
    return pack_contiguous(px, out);
  }
  if format != TextureFormat::Rgba8 {
    for p in px.chunks_exact(c) {
      out.extend(p[..take].iter().map(|&v| to8(v)));
    }
    return;
  }
  // rgba8 channel expansion, SIMD-packed 4 pixels at a time
  let n = px.len() / c;
  let n4 = n & !3;
  let mut buf = [0f32; 16];
  for i in (0..n4).step_by(4) {
    expand_rgba_f32(&px[i * c..(i + 4) * c], c, &mut buf);
    out.extend_from_slice(&pack16(&buf));
  }
  let rem = n - n4;
  if rem > 0 {
    expand_rgba_f32(&px[n4 * c..], c, &mut buf[..rem * 4]);
    out.extend(buf[..rem * 4].iter().map(|&v| to8(v)));
  }
}

/// RGBA f32 expansion with the same channel semantics as `Rgba8` encoding (gray → rgb,
/// b zero-filled, a=1). Only needed for 3ch: other counts upload GPU-direct as R/RG/RGBA32F.
pub fn expand_rgba_f32(px: &[f32], channels: usize, out: &mut [f32]) {
  let c = channels;
  if c == 4 {
    out.copy_from_slice(px);
    return;
  }
  let g_off = (c >= 2) as usize;
  for (p, o) in px.chunks_exact(c).zip(out.chunks_exact_mut(4)) {
    o[0] = p[0];
    o[1] = p[g_off];
    o[2] = if c >= 3 {
      p[2]
    } else if c == 1 {
      p[0]
    } else {
      0.
    };
    o[3] = 1.;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn matches_js_encode_semantics() {
    let enc = |px: &[f32], c: usize, f: TextureFormat| {
      let mut out = Vec::new();
      encode_unorm8(px, c, f, &mut out);
      out
    };
    // clamp, NaN→0, round-half-up
    assert_eq!(
      enc(&[-1., 0., 0.5, 2., f32::NAN, 1., 0.001, 0.999], 4, TextureFormat::Rgba8),
      vec![0, 0, 128, 255, 0, 255, 0, 255]
    );
    // 1ch → gray replicated to rgb, a=255
    assert_eq!(enc(&[0.5], 1, TextureFormat::Rgba8), vec![128, 128, 128, 255]);
    // 2ch → b zero-filled
    assert_eq!(enc(&[1., 0.5], 2, TextureFormat::Rgba8), vec![255, 128, 0, 255]);
    // 3ch → a=255
    assert_eq!(enc(&[0.25, 0.5, 1.], 3, TextureFormat::Rgba8), vec![64, 128, 255, 255]);
    assert_eq!(enc(&[0.5, 1.], 2, TextureFormat::Rg8), vec![128, 255]);
    assert_eq!(enc(&[0.5], 1, TextureFormat::R8), vec![128]);
    // >16-element contiguous path + remainder
    let px: Vec<f32> = (0..37).map(|i| i as f32 / 36.).collect();
    let expected: Vec<u8> = px.iter().map(|&v| to8(v)).collect();
    assert_eq!(enc(&px, 1, TextureFormat::R8), expected);
  }

  #[test]
  fn expand_rgba() {
    let mut out = [0f32; 8];
    expand_rgba_f32(&[0.25, 0.5, 0.75, 0.1, 0.2, 0.3], 3, &mut out);
    assert_eq!(out, [0.25, 0.5, 0.75, 1., 0.1, 0.2, 0.3, 1.]);
    let mut out = [0f32; 4];
    expand_rgba_f32(&[0.5], 1, &mut out);
    assert_eq!(out, [0.5, 0.5, 0.5, 1.]);
    expand_rgba_f32(&[0.5, 0.25], 2, &mut out);
    assert_eq!(out, [0.5, 0.25, 0., 1.]);
  }
}
