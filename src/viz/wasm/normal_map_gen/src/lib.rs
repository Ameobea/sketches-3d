#[cfg(all(feature = "simd", target_arch = "wasm32"))]
use std::arch::wasm32::*;

fn normalize(a: f32, b: f32, c: f32) -> [f32; 3] {
  let len = (a * a + b * b + c * c).sqrt();
  [a / len, b / len, c / len]
}

fn read_interpolated_bilinear_f32(
  texture: &[f32],
  width: usize,
  height: usize,
  x: f32,
  y: f32,
) -> [f32; 3] {
  let x = x.max(0.0).min(width as f32 - 2.0);
  let y = y.max(0.0).min(height as f32 - 2.0);
  let x0 = x.floor() as usize;
  let y0 = y.floor() as usize;
  let x1 = x0 + 1;
  let y1 = y0 + 1;
  let x_ratio = x - x0 as f32;
  let y_ratio = y - y0 as f32;

  let (x0y0, x1y0, x0y1, x1y1) = if cfg!(debug_assertions) {
    let x0y0 = [
      texture[(y0 * width + x0) * 4],
      texture[(y0 * width + x0) * 4 + 1],
      texture[(y0 * width + x0) * 4 + 2],
    ];
    let x1y0 = [
      texture[(y0 * width + x1) * 4],
      texture[(y0 * width + x1) * 4 + 1],
      texture[(y0 * width + x1) * 4 + 2],
    ];
    let x0y1 = [
      texture[(y1 * width + x0) * 4],
      texture[(y1 * width + x0) * 4 + 1],
      texture[(y1 * width + x0) * 4 + 2],
    ];
    let x1y1 = [
      texture[(y1 * width + x1) * 4],
      texture[(y1 * width + x1) * 4 + 1],
      texture[(y1 * width + x1) * 4 + 2],
    ];
    (x0y0, x1y0, x0y1, x1y1)
  } else {
    let x0y0 = unsafe {
      [
        *texture.get_unchecked((y0 * width + x0) * 4),
        *texture.get_unchecked((y0 * width + x0) * 4 + 1),
        *texture.get_unchecked((y0 * width + x0) * 4 + 2),
      ]
    };
    let x1y0 = unsafe {
      [
        *texture.get_unchecked((y0 * width + x1) * 4),
        *texture.get_unchecked((y0 * width + x1) * 4 + 1),
        *texture.get_unchecked((y0 * width + x1) * 4 + 2),
      ]
    };
    let x0y1 = unsafe {
      [
        *texture.get_unchecked((y1 * width + x0) * 4),
        *texture.get_unchecked((y1 * width + x0) * 4 + 1),
        *texture.get_unchecked((y1 * width + x0) * 4 + 2),
      ]
    };
    let x1y1 = unsafe {
      [
        *texture.get_unchecked((y1 * width + x1) * 4),
        *texture.get_unchecked((y1 * width + x1) * 4 + 1),
        *texture.get_unchecked((y1 * width + x1) * 4 + 2),
      ]
    };
    (x0y0, x1y0, x0y1, x1y1)
  };

  let x0y0_ratio = 1.0 - x_ratio;
  let x1y0_ratio = x_ratio;
  let x0y1_ratio = 1.0 - y_ratio;
  let x1y1_ratio = y_ratio;
  [
    x0y0_ratio * x0y0[0] + x1y0_ratio * x1y0[0] + x0y1_ratio * x0y1[0] + x1y1_ratio * x1y1[0],
    x0y0_ratio * x0y0[1] + x1y0_ratio * x1y0[1] + x0y1_ratio * x0y1[1] + x1y1_ratio * x1y1[1],
    x0y0_ratio * x0y0[2] + x1y0_ratio * x1y0[2] + x0y1_ratio * x0y1[2] + x1y1_ratio * x1y1[2],
  ]
}

#[cfg(all(feature = "simd", target_arch = "wasm32"))]
fn read_interpolated_bilinear_f32_simd(
  texture: &[f32],
  width: usize,
  height: usize,
  x: f32,
  y: f32,
) -> [f32; 3] {
  let x = x.min(width as f32 - 2.0);
  let y = y.min(height as f32 - 2.0);
  let x0 = x.floor() as usize;
  let y0 = y.floor() as usize;
  let x1 = x0 + 1;
  let y1 = y0 + 1;
  let x_ratio = x - x0 as f32;
  let y_ratio = y - y0 as f32;

  let texptr = texture.as_ptr();
  let x0y0 = unsafe {
    let x0y0_ratio = f32x4_splat(1. - x_ratio);
    f32x4_mul(
      x0y0_ratio,
      v128_load(texptr.add((y0 * width + x0) * 4) as *const _),
    )
  };
  let x1y0 = unsafe {
    let x1y0_ratio = f32x4_splat(x_ratio);
    f32x4_mul(
      x1y0_ratio,
      v128_load(texptr.add((y0 * width + x1) * 4) as *const _),
    )
  };
  let x0y1 = unsafe {
    let x0y1_ratio = f32x4_splat(1. - y_ratio);
    f32x4_mul(
      x0y1_ratio,
      v128_load(texptr.add((y1 * width + x0) * 4) as *const _),
    )
  };
  let x1y1 = unsafe {
    let x1y1_ratio = f32x4_splat(y_ratio);
    f32x4_mul(
      x1y1_ratio,
      v128_load(texptr.add((y1 * width + x1) * 4) as *const _),
    )
  };

  let out = f32x4_add(f32x4_add(x0y0, x1y0), f32x4_add(x0y1, x1y1));
  [
    f32x4_extract_lane::<0>(out),
    f32x4_extract_lane::<1>(out),
    f32x4_extract_lane::<2>(out),
  ]
}

fn magnitude(v: [f32; 3]) -> f32 {
  (v[0] + v[1] + v[2]) / 3.0
}

#[no_mangle]
pub extern "C" fn malloc(size: usize) -> *mut u8 {
  let mut v = Vec::with_capacity(size);
  let ptr = v.as_mut_ptr();
  std::mem::forget(v);
  ptr
}

#[no_mangle]
pub extern "C" fn free(ptr: *mut u8) {
  unsafe {
    let _ = Vec::from_raw_parts(ptr, 0, 0);
  }
}

/// Expect texture in RGBA format.  Returns normal map in RGBA format.
///
/// Adapted from code by Jan Frischmuth <http://www.smart-page.net/blog>
#[no_mangle]
pub extern "C" fn gen_normal_map_from_texture(
  texture: *const u8,
  height: usize,
  width: usize,
  pack_mode: u8,
) -> *mut u8 {
  let texture = unsafe { std::slice::from_raw_parts(texture, height * width * 4) };
  let texture_f32 = texture
    .iter()
    .map(|&x| x as f32 / 255.0)
    .collect::<Vec<_>>();

  enum PackMode {
    /// No packing is done; the returned texture contains all 3 components of
    /// the normal vector with 1 set for the alpha channel.
    None,
    /// The texture data is assumed to be in grayscale.  The returned RGBA
    /// texture will have the normal vector packed into the GBA channels with
    /// the provided texture data into the R channel.
    GrayScaleGBA,
  }

  let pack_mode = match pack_mode {
    0 => PackMode::None,
    1 => PackMode::GrayScaleGBA,
    _ => panic!("Invalid pack mode"),
  };

  let pixel_count = texture.len() / 4;
  let mut normal_map = Vec::with_capacity(pixel_count * 4);

  let step_x = 1.0 / width as f32;
  let step_y = 1.0 / height as f32;

  for y in 0..height {
    for x in 0..width {
      let d0 = [
        texture_f32[(y * width + x) * 4],
        texture_f32[(y * width + x) * 4 + 1],
        texture_f32[(y * width + x) * 4 + 2],
      ];
      #[cfg(all(feature = "simd", target_arch = "wasm32"))]
      let (d1, d2, d3, d4) = {
        let d1 = read_interpolated_bilinear_f32_simd(
          &texture_f32,
          width,
          height,
          x as f32 + step_x,
          y as f32,
        );
        let d2 = read_interpolated_bilinear_f32_simd(
          &texture_f32,
          width,
          height,
          x as f32 - step_x,
          y as f32,
        );
        let d3 = read_interpolated_bilinear_f32_simd(
          &texture_f32,
          width,
          height,
          x as f32,
          y as f32 + step_y,
        );
        let d4 = read_interpolated_bilinear_f32_simd(
          &texture_f32,
          width,
          height,
          x as f32,
          y as f32 - step_y,
        );
        (d1, d2, d3, d4)
      };

      #[cfg(not(all(feature = "simd", target_arch = "wasm32")))]
      let (d1, d2, d3, d4) = {
        let d1 =
          read_interpolated_bilinear_f32(&texture_f32, width, height, x as f32 + step_x, y as f32);
        let d2 =
          read_interpolated_bilinear_f32(&texture_f32, width, height, x as f32 - step_x, y as f32);
        let d3 =
          read_interpolated_bilinear_f32(&texture_f32, width, height, x as f32, y as f32 + step_y);
        let d4 =
          read_interpolated_bilinear_f32(&texture_f32, width, height, x as f32, y as f32 - step_y);
        (d1, d2, d3, d4)
      };

      let dx = ((magnitude(d2) - magnitude(d0)) + (magnitude(d0) - magnitude(d1))) * 0.5;
      let dy = ((magnitude(d4) - magnitude(d0)) + (magnitude(d0) - magnitude(d3))) * 0.5;

      let bias = 0.1;
      let normal = normalize(dx, dy, 1.0 - ((bias - 0.1) / 100.0));
      let normal = [
        normal[0] * 0.5 + 0.5,
        normal[1] * 0.5 + 0.5,
        normal[2] * 0.5 + 0.5,
      ];

      match pack_mode {
        PackMode::None => {
          normal_map.push((normal[0] * 255.0) as u8);
          normal_map.push((normal[1] * 255.0) as u8);
          normal_map.push((normal[2] * 255.0) as u8);
          normal_map.push(255);
        }
        PackMode::GrayScaleGBA => {
          normal_map.push(texture[(y * width + x) * 4]);
          normal_map.push((normal[0] * 255.0) as u8);
          normal_map.push((normal[1] * 255.0) as u8);
          normal_map.push((normal[2] * 255.0) as u8);
        }
      }
    }
  }

  let ptr = normal_map.as_mut_ptr();
  std::mem::forget(normal_map);
  ptr
}
