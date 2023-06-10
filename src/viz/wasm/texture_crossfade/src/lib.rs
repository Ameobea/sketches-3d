use std::ptr;

static mut TEXTURE_PTRS: [*mut u8; 8] = [ptr::null_mut(); 8];

#[no_mangle]
pub extern "C" fn wasm_malloc(size: usize) -> *mut u8 {
  let mut data = Vec::with_capacity(size);
  let ptr = data.as_mut_ptr();
  std::mem::forget(data);
  ptr
}

#[no_mangle]
pub extern "C" fn wasm_free(ptr: *mut u8) {
  unsafe {
    drop(Box::from_raw(ptr));
  }
}

#[no_mangle]
pub extern "C" fn set_texture(data: *mut u8, index: usize) {
  unsafe {
    TEXTURE_PTRS[index] = data;
  }
}

#[no_mangle]
pub extern "C" fn reset() {
  unsafe {
    for i in 0..TEXTURE_PTRS.len() {
      if !TEXTURE_PTRS[i].is_null() {
        drop(Box::from_raw(TEXTURE_PTRS[i]));
        TEXTURE_PTRS[i] = ptr::null_mut();
      }
    }
  }
}

/// Projects the coordinates from [0, 0], [1, 1] relative to the current tile to
/// coordinates relative to a corner of a tile from [-1, -1], [1, 1] within
/// `threshold` of that corner.
fn project_box_coord(x: f32, y: f32, threshold: f32) -> (f32, f32, i8, i8) {
  let half_threshold = threshold / 2.;

  let x_side;
  let y_side;

  let normalized_x = if x < half_threshold {
    x_side = -1;
    x / half_threshold
  } else if x > 1. - half_threshold {
    x_side = 1;
    (x - 1.) / half_threshold
  } else {
    if x < 0.5 {
      x_side = -1;
      1.
    } else {
      x_side = 1;
      -1.
    }
  };
  let normalized_y = if y < half_threshold {
    y_side = -1;
    y / half_threshold
  } else if y > 1. - half_threshold {
    y_side = 1;
    (y - 1.) / half_threshold
  } else {
    if y < 0.5 {
      y_side = -1;
      1.
    } else {
      y_side = 1;
      -1.
    }
  };

  (normalized_x, normalized_y, x_side, y_side)
}

#[test]
fn project_box_coord_correctness() {
  let threshold = 0.2;
  assert_eq!(project_box_coord(0., 0., threshold), (0., 0., -1, -1));
  assert_eq!(project_box_coord(1., 1., threshold), (0., 0., 1, 1));
  assert_eq!(project_box_coord(0.5, 0.5, threshold), (-1., -1., 1, 1));
  assert_eq!(
    project_box_coord(0.05, 0.95, threshold),
    (0.5, -0.5000001, -1, 1)
  );
}

/// Returns the indices around the current corneras  (top left, top right,
/// bottom left, bottom right)
fn get_texture_indices_for_corner(
  texture_count: usize,
  base_texture_ix: usize,
  x_side: i8,
  y_side: i8,
) -> (usize, usize, usize, usize) {
  let get_prev_ix = |cur_ix: usize| {
    if cur_ix == 0 {
      texture_count - 1
    } else {
      cur_ix - 1
    }
  };
  let get_next_ix = |cur_ix: usize| {
    if cur_ix == texture_count - 1 {
      0
    } else {
      cur_ix + 1
    }
  };

  let prev_ix = get_prev_ix(base_texture_ix as usize);
  let prev_prev_ix = get_prev_ix(prev_ix);
  let next_ix = get_next_ix(base_texture_ix as usize);
  let next_next_ix = get_next_ix(next_ix);

  match (x_side, y_side) {
    (-1, -1) => (prev_prev_ix, prev_ix, prev_ix, base_texture_ix),
    (-1, 1) => (prev_ix, base_texture_ix, base_texture_ix, next_ix),
    (1, -1) => (prev_ix, base_texture_ix, base_texture_ix, next_ix),
    (1, 1) => (base_texture_ix, next_ix, next_ix, next_next_ix),
    _ => unreachable!(),
  }
}

#[no_mangle]
pub extern "C" fn generate(size: usize, threshold: f32) -> *mut u8 {
  if threshold < 0. || threshold > 1. {
    panic!("Threshold must be between 0 and 1");
  }

  let textures = unsafe { &TEXTURE_PTRS }
    .iter()
    .take_while(|&ptr| !ptr.is_null())
    .map(|&data| unsafe { std::slice::from_raw_parts_mut(data, size * size * 4) })
    .collect::<Vec<_>>();

  // textures count must be a power of 2
  if textures.len().count_ones() != 1 {
    panic!("Textures count must be a power of 2");
  }

  // DEBUG
  // for y in 0..size {
  //   for x in 0..size {
  //     for (i, color) in [
  //       (0usize, [255, 0, 0]),
  //       (1, [0, 255, 0]),
  //       (2, [0, 0, 255]),
  //       (3, [255, 255, 0]),
  //     ] {
  //       let texture = &mut textures[i];
  //       let magnitude = ((x as f32 / size as f32) * (y as f32 / size as f32)) /
  // 2.;       texture[y * size * 4 + x * 4 + 0] = (magnitude * color[0] as f32)
  // as u8;       texture[y * size * 4 + x * 4 + 1] = (magnitude * color[1] as
  // f32) as u8;       texture[y * size * 4 + x * 4 + 2] = (magnitude * color[2]
  // as f32) as u8;     }
  //   }
  // }

  // for chunk in textures[0].chunks_mut(4) {
  //   chunk[0] = 255;
  //   chunk[1] = 0;
  //   chunk[2] = 0;
  //   chunk[3] = 255;
  // }
  // for chunk in textures[1].chunks_mut(4) {
  //   chunk[0] = 0;
  //   chunk[1] = 255;
  //   chunk[2] = 0;
  //   chunk[3] = 255;
  // }
  // for chunk in textures[2].chunks_mut(4) {
  //   chunk[0] = 0;
  //   chunk[1] = 0;
  //   chunk[2] = 255;
  //   chunk[3] = 255;
  // }
  // for chunk in textures[3].chunks_mut(4) {
  //   chunk[0] = 0;
  //   chunk[1] = 0;
  //   chunk[2] = 0;
  //   chunk[3] = 255;
  // }
  // END DEBUG

  let out_size = size * textures.len();
  let mut out: Vec<u8> = Vec::with_capacity(out_size * out_size * 4);
  for y in 0..out_size {
    let y_cur_tile_progress = (y % size) as f32 / size as f32;
    let y_cur_tile = y / size;

    for x in 0..out_size {
      let x_cur_tile_progress = (x % size) as f32 / size as f32;
      let x_cur_tile = x / size;
      let base_tx_ix = (x_cur_tile + y_cur_tile) % textures.len();
      let base_texture_ix = {
        let x = x % size;
        let y = y % size;
        y * size * 4 + x * 4
      };

      let (normalized_x, normalized_y, x_side, y_side) =
        match project_box_coord(x_cur_tile_progress, y_cur_tile_progress, threshold) {
          o => o,
        };
      let normalized_x = (normalized_x + 1.) / 2.;
      let normalized_y = (normalized_y + 1.) / 2.;

      let (top_left_ix, top_right_ix, bot_left_ix, bot_right_ix) =
        get_texture_indices_for_corner(textures.len(), base_tx_ix, x_side, y_side);
      let top_left_texture = &*textures[top_left_ix];
      let top_left_sample = [
        top_left_texture[base_texture_ix + 0] as f32, // * tl_weight,
        top_left_texture[base_texture_ix + 1] as f32, // * tl_weight,
        top_left_texture[base_texture_ix + 2] as f32, // * tl_weight,
      ];
      let top_right_texture = &*textures[top_right_ix];
      let top_right_sample = [
        top_right_texture[base_texture_ix + 0] as f32, // * tr_weight,
        top_right_texture[base_texture_ix + 1] as f32, // * tr_weight,
        top_right_texture[base_texture_ix + 2] as f32, // * tr_weight,
      ];
      let bot_left_texture = &*textures[bot_left_ix];
      let bot_left_sample = [
        bot_left_texture[base_texture_ix + 0] as f32, // * bl_weight,
        bot_left_texture[base_texture_ix + 1] as f32, // * bl_weight,
        bot_left_texture[base_texture_ix + 2] as f32, // * bl_weight,
      ];
      let bot_right_texture = &*textures[bot_right_ix];
      let bot_right_sample = [
        bot_right_texture[base_texture_ix + 0] as f32, // * br_weight,
        bot_right_texture[base_texture_ix + 1] as f32, // * br_weight,
        bot_right_texture[base_texture_ix + 2] as f32, // * br_weight,
      ];

      // bilinear interpolation
      let top_sample = [
        top_left_sample[0] * (1. - normalized_x) + top_right_sample[0] * normalized_x,
        top_left_sample[1] * (1. - normalized_x) + top_right_sample[1] * normalized_x,
        top_left_sample[2] * (1. - normalized_x) + top_right_sample[2] * normalized_x,
      ];
      let bot_sample = [
        bot_left_sample[0] * (1. - normalized_x) + bot_right_sample[0] * normalized_x,
        bot_left_sample[1] * (1. - normalized_x) + bot_right_sample[1] * normalized_x,
        bot_left_sample[2] * (1. - normalized_x) + bot_right_sample[2] * normalized_x,
      ];
      let sample = [
        top_sample[0] * (1. - normalized_y) + bot_sample[0] * normalized_y,
        top_sample[1] * (1. - normalized_y) + bot_sample[1] * normalized_y,
        top_sample[2] * (1. - normalized_y) + bot_sample[2] * normalized_y,
      ];

      out.push(sample[0] as u8);
      out.push(sample[1] as u8);
      out.push(sample[2] as u8);
      out.push(255);
    }
  }

  Box::into_raw(out.into_boxed_slice()) as *mut u8
}
