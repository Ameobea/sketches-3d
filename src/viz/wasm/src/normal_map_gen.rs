use wasm_bindgen::prelude::*;

fn read_interpolated_bilinear(
  texture: &[u8],
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
  let x0y0 = [
    texture[(y0 * width + x0) * 4] as f32 / 255.0,
    texture[(y0 * width + x0) * 4 + 1] as f32 / 255.0,
    texture[(y0 * width + x0) * 4 + 2] as f32 / 255.0,
  ];
  let x1y0 = [
    texture[(y0 * width + x1) * 4] as f32 / 255.0,
    texture[(y0 * width + x1) * 4 + 1] as f32 / 255.0,
    texture[(y0 * width + x1) * 4 + 2] as f32 / 255.0,
  ];
  let x0y1 = [
    texture[(y1 * width + x0) * 4] as f32 / 255.0,
    texture[(y1 * width + x0) * 4 + 1] as f32 / 255.0,
    texture[(y1 * width + x0) * 4 + 2] as f32 / 255.0,
  ];
  let x1y1 = [
    texture[(y1 * width + x1) * 4] as f32 / 255.0,
    texture[(y1 * width + x1) * 4 + 1] as f32 / 255.0,
    texture[(y1 * width + x1) * 4 + 2] as f32 / 255.0,
  ];
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

fn magnitude(v: [f32; 3]) -> f32 {
  (v[0] + v[1] + v[2]) / 3.0
}

/// Expect texture in RGBA format.  Returns normal map in RGBA format.
///
/// Adapted from code by Jan Frischmuth <http://www.smart-page.net/blog>
#[wasm_bindgen]
pub fn gen_normal_map_from_texture(texture: &[u8], height: usize, width: usize) -> Vec<u8> {
  let pixel_count = texture.len() / 4;
  let mut normal_map = Vec::with_capacity(pixel_count * 4);

  let step_x = 1.0 / width as f32;
  let step_y = 1.0 / height as f32;

  for y in 0..height {
    for x in 0..width {
      let d0 = [
        texture[(y * width + x) * 4] as f32 / 255.0,
        texture[(y * width + x) * 4 + 1] as f32 / 255.0,
        texture[(y * width + x) * 4 + 2] as f32 / 255.0,
      ];
      let d1 = read_interpolated_bilinear(texture, width, height, x as f32 + step_x, y as f32);
      let d2 = read_interpolated_bilinear(texture, width, height, x as f32 - step_x, y as f32);
      let d3 = read_interpolated_bilinear(texture, width, height, x as f32, y as f32 + step_y);
      let d4 = read_interpolated_bilinear(texture, width, height, x as f32, y as f32 - step_y);
      let dx = ((magnitude(d2) - magnitude(d0)) + (magnitude(d0) - magnitude(d1))) * 0.5;
      let dy = ((magnitude(d4) - magnitude(d0)) + (magnitude(d0) - magnitude(d3))) * 0.5;

      let bias = 0.1;
      let normal = nalgebra::Vector3::new(dx, dy, 1.0 - ((bias - 0.1) / 100.0));
      let normal = normal.normalize();
      let normal = normal * 0.5;
      let normal = normal + nalgebra::Vector3::new(0.5, 0.5, 0.5);
      normal_map.push((normal[0] * 255.0) as u8);
      normal_map.push((normal[1] * 255.0) as u8);
      normal_map.push((normal[2] * 255.0) as u8);
      normal_map.push(255);
    }
  }

  normal_map
}
